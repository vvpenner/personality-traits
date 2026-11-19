use std::path::PathBuf;

#[path = "../src/events.rs"]
mod events;

#[test]
fn loads_real_yaml() {
    let yaml = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("perkeys.yaml");
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let course = events::Course::load(&yaml, &data).expect("perkeys.yaml must parse");
    assert!(!course.events.is_empty(), "course must have events");

    // Стартовое событие с кнопкой «Начать курс».
    let start = course.first().expect("has first event");
    assert_eq!(start.id, "start");
    assert!(
        !start.buttons.is_empty(),
        "start event must have a button to begin"
    );

    // Все 28 дней присутствуют целиком: info -> practice -> outro.
    for day in 1..=28 {
        for part in ["info", "practice", "outro"] {
            let id = format!("day_{}_{}", day, part);
            assert!(course.index_of(&id).is_some(), "{} must exist", id);
        }
    }

    // Еженедельные опросы после дней 7, 14, 21, 28.
    for day in [7, 14, 21, 28] {
        let outro = course
            .index_of(&format!("day_{}_outro", day))
            .expect("outro exists");
        let survey = course
            .index_of(&format!("survey_{}", day))
            .expect("survey exists");
        let target = course
            .next_after_button(outro, &format!("survey_{}", day))
            .expect("outro button leads to survey");
        assert_eq!(target, survey, "day_{} outro must lead to its survey", day);
    }

    // У опросов есть url-кнопка и кнопка «Опрос пройден».
    for day in [7, 14, 21, 28] {
        let survey = course
            .at(course.index_of(&format!("survey_{}", day)).unwrap())
            .unwrap();
        assert!(
            survey.buttons.iter().any(|b| !b.url.is_empty()),
            "survey_{} must have a url button",
            day
        );
        assert!(
            survey.buttons.iter().any(|b| !b.event.is_empty()),
            "survey_{} must have a scenario button",
            day
        );
    }

    // Пост-тест в конце сценария.
    assert!(course.index_of("posttest").is_some());
    assert!(course.index_of("posttest_after").is_some());

    // Цепочка день 1: кнопка ведёт на практику, практика — на выходное сообщение.
    let info1 = course.index_of("day_1_info").unwrap();
    let practice1 = course.index_of("day_1_practice").unwrap();
    let outro1 = course.index_of("day_1_outro").unwrap();
    assert_eq!(
        course.next_after_button(info1, "day_1_practice"),
        Some(practice1)
    );
    assert_eq!(
        course.next_after_button(practice1, "day_1_outro"),
        Some(outro1)
    );
}

#[test]
fn loads_central_yaml() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let yaml = root.join("central.yaml");
    let data = root.join("questionnaire/abfi-1/data");
    let course = events::Course::load(&yaml, &data).expect("central.yaml must parse");
    assert_eq!(course.events.len(), 96, "central course must have 96 events");
    assert_eq!(course.first().map(|e| e.id.as_str()), Some("start"));

    for day in 1..=28 {
        for part in ["info", "practice", "outro"] {
            let id = format!("day_{}_{}", day, part);
            assert!(course.index_of(&id).is_some(), "{} must exist", id);
        }
    }
    for id in [
        "survey_7",
        "survey_14",
        "survey_21",
        "survey_28",
        "finish",
        "posttest",
        "posttest_survey",
        "posttest_after",
    ] {
        assert!(course.index_of(id).is_some(), "{} must exist", id);
    }
}
