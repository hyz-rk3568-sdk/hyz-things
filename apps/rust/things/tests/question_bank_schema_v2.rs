#![cfg(feature = "native")]
use hyz_things::application::question_bank::document::parse;

const V2_SHENLUN: &str = r#"---
schema: hyz-paper/v2
title: 2026年国考申论（副省级）
year: 2026
region: 国考
variant: 副省级
subject: shenlun
role: primary
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
# 2026年国考申论（副省级）
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b610 -->
给定材料。
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
answer_type: subjective
question_type: summary
grading_mode: ai
max_score: 10
materials: [M1]
```
### Stem
请根据给定资料，概括 H 市管理“城市用光”的做法。
### Options

### Answer

### Reference Answer
参考答案。
### Explanation
解析。
"#;

#[test]
fn v2_shenlun_paper_preserves_subject_and_question_attributes() {
    let paper = parse(V2_SHENLUN).unwrap();
    let view = paper.view();
    assert_eq!(view.subject, "shenlun");
    assert_eq!(view.questions.len(), 1);
    assert_eq!(view.questions[0].answer_type, "subjective");
    assert_eq!(view.questions[0].question_type.as_deref(), Some("summary"));
    assert_eq!(view.questions[0].grading_mode, "ai");
    assert_eq!(view.questions[0].max_score, Some(10.0));
    assert_eq!(
        view.questions[0].materials,
        vec![view.materials[0].id.clone()]
    );
    assert_eq!(paper.summary().subject, "shenlun");
    let key = paper.key("019a3d8e-1b32-7a60-9c83-f34217a6e210").unwrap();
    assert_eq!(key.reference_answer, "参考答案。");
    assert!(key.choices.is_empty());
    assert!(!key.gradable);
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("参考答案"));
    assert!(!json.contains("reference_answer"));
}

#[test]
fn v1_paper_defaults_to_xingce_objective_exact() {
    let paper = parse(
        r#"---
schema: hyz-paper/v1
title: 旧卷
year: 2020
region: 国考
variant: 统考
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
# 旧卷
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
module: 资料分析
```
### Stem
题干。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析。
"#,
    )
    .unwrap();
    let view = paper.view();
    assert_eq!(view.subject, "xingce");
    assert_eq!(view.questions[0].answer_type, "objective");
    assert_eq!(view.questions[0].grading_mode, "exact");
    assert_eq!(view.questions[0].question_type, None);
    assert_eq!(view.questions[0].max_score, None);
}

#[test]
fn v2_requires_subject() {
    let without_subject = V2_SHENLUN.replace("subject: shenlun\n", "");
    assert_eq!(parse(&without_subject).unwrap_err(), "missing subject");
}

#[test]
fn v2_subjective_requires_reference_answer_section() {
    let without_reference = V2_SHENLUN.replace("### Reference Answer\n参考答案。\n", "");
    assert_eq!(
        parse(&without_reference).unwrap_err(),
        "missing Reference Answer"
    );
}

#[test]
fn v2_rejects_unknown_grading_mode() {
    let invalid = V2_SHENLUN.replace("grading_mode: ai", "grading_mode: mystery");
    assert_eq!(parse(&invalid).unwrap_err(), "unsupported grading_mode");
}

#[test]
fn v2_rejects_non_positive_max_score() {
    let invalid = V2_SHENLUN.replace("max_score: 10", "max_score: 0");
    assert_eq!(parse(&invalid).unwrap_err(), "invalid max_score");
}

#[tokio::test]
async fn sqlite_round_trips_shenlun_attributes_and_excludes_practice() {
    use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
    use hyz_things::application::question_bank::{IndexedPaper, QuestionBankRepositoryPort};

    let repository = SqliteQuestionBankRepository::connect("sqlite::memory:")
        .await
        .unwrap();
    let indexed = IndexedPaper::new(
        "papers/国考/2026/申论-副省级.md".into(),
        "hash".into(),
        parse(V2_SHENLUN).unwrap(),
    );
    repository.apply(vec![indexed]).await.unwrap();

    let summaries = repository.paper_summaries().await.unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].subject, "shenlun");
    let content = repository.paper_content(&summaries[0].id).await.unwrap();
    let question = &content.questions[0];
    assert_eq!(question.answer_type, "subjective");
    assert_eq!(question.question_type.as_deref(), Some("summary"));
    assert_eq!(question.grading_mode, "ai");
    assert_eq!(question.max_score, Some(10.0));
    assert_eq!(question.reference_answer, "参考答案。");
    assert!(!question.gradable);

    let catalog = repository
        .practice_catalog(&Default::default())
        .await
        .unwrap();
    assert!(catalog.modules.is_empty());
}
