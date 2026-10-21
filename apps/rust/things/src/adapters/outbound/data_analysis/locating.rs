//! Fresh simulated passages with five numeric-location tasks.
//! Existing runs remain durable, never reused as new exercises.
use super::*;
use std::collections::{BTreeMap, HashSet};

const LOCATING_COUNT: usize = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocatingRequest {
    pub topic: Option<String>,
    pub subtopic: Option<String>,
    #[serde(default)]
    pub allow_review: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocatingTopic {
    pub topic: String,
    pub subtopics: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LocatingRecipe {
    question: String,
    subject: String,
    indicator: String,
    anchor: String,
    target: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LocatingTarget {
    question: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocatingAnswer {
    pub correct: bool,
    pub elapsed_ms: u32,
    pub selected_start: usize,
    pub expected_start: usize,
}

#[derive(Clone, Serialize)]
pub struct LocatingPublicQuestion {
    pub question: String,
}

#[derive(Clone, Serialize)]
pub struct LocatingView {
    pub id: String,
    pub status: String,
    pub material: Material,
    pub questions: Vec<LocatingPublicQuestion>,
    pub answers: Vec<Option<LocatingAnswer>>,
}

#[derive(Clone, Deserialize)]
pub struct LocateClick {
    pub start: usize,
    pub end: usize,
    pub elapsed_ms: u32,
}

pub(super) fn material_key(material: &Material) -> String {
    // Old official-derived papers preserve their historical exposure identity.
    // Simulated scenarios are independent even when based on the same official
    // chapter. Repeat exposures of identical scenarios still share a key.
    if material
        .text
        .starts_with("【模拟训练数据，非官方发布数值】")
    {
        let digest = sha2::Sha256::digest(material.text.as_bytes());
        let hash = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        return format!("simulated\u{1f}{hash}");
    }
    format!("{}\u{1f}{}", material.source_url, material.section)
}

fn locator_is_numeric(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    !chars.is_empty()
        && chars.len() <= 24
        && chars.first().is_some_and(char::is_ascii_digit)
        && chars.last().is_some_and(char::is_ascii_digit)
        && chars
            .iter()
            .all(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
}

fn is_number_character(c: char) -> bool {
    c.is_ascii_digit() || c == '.' || c == ','
}

/// Anchor must occur exactly once, contain a unique complete numeric token,
/// and bind the same subject and indicator used in the question.
fn verify_locators(
    material: &Material,
    recipes: &[LocatingRecipe],
) -> Result<Vec<LocatingTarget>, String> {
    if recipes.len() != LOCATING_COUNT {
        return Err("找数题数量不是5题".into());
    }
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for recipe in recipes {
        if recipe.question.chars().count() < 12
            || recipe.question.chars().count() > 180
            || recipe.subject.trim().is_empty()
            || recipe.indicator.trim().is_empty()
            || !recipe.question.contains(&recipe.subject)
            || !recipe.question.contains(&recipe.indicator)
            || !recipe.anchor.contains(&recipe.subject)
            || !recipe.anchor.contains(&recipe.indicator)
            || !locator_is_numeric(&recipe.target)
        {
            return Err("题干或主体、指标没有与材料对应".into());
        }
        let Some(anchor_offset) = material.text.find(&recipe.anchor) else {
            return Err("定位证据未在改写材料中出现".into());
        };
        if material.text[anchor_offset + recipe.anchor.len()..].contains(&recipe.anchor) {
            return Err("定位证据存在重复片段".into());
        }
        let Some(within_anchor) = recipe.anchor.find(&recipe.target) else {
            return Err("目标数字不在定位证据中".into());
        };
        if recipe.anchor[within_anchor + recipe.target.len()..].contains(&recipe.target) {
            return Err("定位证据中数字有歧义".into());
        }
        let start = anchor_offset + within_anchor;
        let end = start + recipe.target.len();
        let preceding = material.text[..start].chars().next_back();
        let following = material.text[end..].chars().next();
        if preceding.is_some_and(is_number_character) || following.is_some_and(is_number_character)
        {
            return Err("目标不是完整数字".into());
        }
        if !seen.insert(start) {
            return Err("五道题引用了相同位置".into());
        }
        targets.push(LocatingTarget {
            question: recipe.question.clone(),
            start,
            end,
        });
    }
    Ok(targets)
}

impl DataAnalysisService {
    /// Existing comprehensive sessions predate shared material exposure tracking.
    /// Backfill only sessions actually started, not generated but unseen papers.
    pub(super) async fn sync_seen_materials(&self) -> Result<HashSet<String>, String> {
        let rows =
            sqlx::query("SELECT groups_json FROM data_analysis_runs WHERE status <> 'ready'")
                .fetch_all(&self.pool)
                .await
                .map_err(as_error)?;
        for row in rows {
            let data: String = row.try_get("groups_json").map_err(as_error)?;
            if let Ok(groups) = serde_json::from_str::<Vec<QuestionGroup>>(&data) {
                for group in groups {
                    self.mark_material_seen(&group.material, "comprehensive")
                        .await?;
                }
            }
        }
        let rows = sqlx::query("SELECT material_key FROM data_analysis_material_seen")
            .fetch_all(&self.pool)
            .await
            .map_err(as_error)?;
        rows.into_iter()
            .map(|r| r.try_get("material_key").map_err(as_error))
            .collect()
    }

    pub(super) async fn mark_material_seen(
        &self,
        material: &Material,
        mode: &str,
    ) -> Result<(), String> {
        sqlx::query("INSERT OR IGNORE INTO data_analysis_material_seen(material_key,source_url,section,first_mode,seen_at_ms) VALUES(?,?,?,?,?)")
            .bind(material_key(material)).bind(&material.source_url).bind(&material.section)
            .bind(mode).bind(now_ms()).execute(&self.pool).await.map_err(as_error)?;
        Ok(())
    }

    pub async fn locating_topics(&self) -> Result<Vec<LocatingTopic>, String> {
        let mut groups = BTreeMap::<String, Vec<String>>::new();
        for (_, topic) in SECTIONS {
            groups.entry(topic.to_owned()).or_default();
        }
        for row in sqlx::query(
            "SELECT group_json FROM data_analysis_groups ORDER BY created_at_ms DESC LIMIT 300",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?
        {
            let data: String = row.try_get("group_json").map_err(as_error)?;
            if let Ok(group) = serde_json::from_str::<QuestionGroup>(&data) {
                let subtopics = groups.entry(group.material.topic).or_default();
                if !subtopics.contains(&group.material.subtopic) {
                    subtopics.push(group.material.subtopic);
                }
            }
        }
        Ok(groups
            .into_iter()
            .map(|(topic, mut subtopics)| {
                subtopics.sort();
                LocatingTopic { topic, subtopics }
            })
            .collect())
    }

    pub async fn prepare_locating(&self, request: LocatingRequest) -> Result<String, String> {
        if self.config.is_none() {
            return Err("Codex Token 未配置".into());
        }
        if request.topic.as_ref().is_some_and(|s| s.len() > 80)
            || request.subtopic.as_ref().is_some_and(|s| s.len() > 80)
        {
            return Err("主题筛选无效".into());
        }
        if self.gate.available_permits() == 0 {
            return Err("已有出题任务正在执行".into());
        }
        let id = Uuid::now_v7().to_string();
        self.jobs.lock().await.insert(
            id.clone(),
            PrepareProgress {
                state: "pending".into(),
                verified: 0,
                message: "正在准备找数材料".into(),
                run_id: None,
            },
        );
        let worker = self.clone();
        let job = id.clone();
        tokio::spawn(async move {
            let Ok(_guard) = worker.gate.clone().acquire_owned().await else {
                return;
            };
            let result = worker.build_locating_run(&job, &request).await;
            match result {
                Ok(id) => {
                    worker
                        .set_progress(&job, "ready", 5, "已生成5道找数题", Some(id))
                        .await
                }
                Err(e) => worker.set_progress(&job, "failed", 0, &e, None).await,
            }
        });
        Ok(id)
    }

    async fn build_locating_run(&self,job:&str,request:&LocatingRequest)->Result<String,String>{
        // Do not use old question-group or locating-set caches for a new attempt.
        self.set_progress(job,"fetching",0,"正在准备新的统计背景",None).await;
        let sources=self.load_sections().await?;
        let candidates=sources.into_iter().filter(|m|matches_filter(m,request)).collect::<Vec<_>>();
        if candidates.is_empty(){return Err("当前主题没有可用统计参考背景".into());}
        let mut chosen=None;
        let mut last_error=String::new();
        for source in candidates.iter().take(4){
            self.set_progress(job,"generating",0,"Codex 正在生成新材料和5道找数题",None).await;
            match self.generate_locators(source).await{
                Ok(batch)=>{chosen=Some(batch);break;}
                Err(e)=>last_error=e,
            }
        }
        let (material,questions)=chosen.ok_or_else(||format!("找数训练生成失败：{last_error}"))?;
        let group_id=Uuid::now_v7().to_string();
        let group=QuestionGroup{version:5,material:material.clone(),questions:Vec::new()};
        sqlx::query("INSERT INTO data_analysis_groups(id,source_url,section,topic,group_json,created_at_ms) VALUES(?,?,?,?,?,?)")
            .bind(&group_id).bind(&material.source_url).bind(&material.section)
            .bind(&material.topic).bind(serde_json::to_string(&group).map_err(as_error)?)
            .bind(now_ms()).execute(&self.pool).await.map_err(as_error)?;
        let id=Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO data_analysis_locating_runs(id,group_id,material_key,questions_json,answers_json,status,created_at_ms) VALUES(?,?,?,?,?,'ready',?)")
            .bind(&id).bind(&group_id).bind(material_key(&material))
            .bind(serde_json::to_string(&questions).map_err(as_error)?)
            .bind(serde_json::to_string(&vec![None::<LocatingAnswer>;LOCATING_COUNT]).map_err(as_error)?)
            .bind(now_ms()).execute(&self.pool).await.map_err(as_error)?;
        Ok(id)
    }

    async fn generate_locators(&self,source:&Material)->Result<(Material,Vec<LocatingTarget>),String>{
        let config=self.config.as_ref().ok_or("Codex Token 未配置")?;
        let mut session=tokio::time::timeout(
            Duration::from_secs(12),CodexSession::connect(&config.token,&config.cwd)
        ).await.map_err(|_|"连接 Codex App Server 超时".to_owned())??;
        let year=OFFICIAL_SOURCES.iter().find(|(_,url)|*url==source.source_url)
            .map(|(year,_)|*year).unwrap_or(2025);
        let system=format!(r#"你是 hyz 的公务员行测资料分析找数专项命题 Agent。
使用 hyz-xingce Skill 和本仓库最新 flashcards/资料/ 中的定位、建立数据地图方法。
本次只训练寻找原文数字，不计算、不设计选择题。
先规划5个定位任务（年份、主体、指标、现期/同比口径），再构造统计模拟数据，
组织成350–650汉字、2～4个自然段的行测仿真材料；material_text 用两个换行符分隔自然段（\\n\\n，JSON中正确转义）。
整篇只围绕一个统计大主题，各段按总体、细分主体或相关指标分别展开，保留合理跨段查找与口径干扰，不拼接无关统计领域。
第1–2题帮助探索不同数据区域，第3–5题复用数据地图，不要机械地按材料顺序给答案。
统计背景年份{year}，主题「{topic}／{subtopic}」；官方摘录只提供主体、指标和统计口径参考，不代表模拟数字属于官方公布：
{excerpt}
只输出JSON对象，含material_text与questions两字段，questions恰好5项。
每题包含question、subject、indicator、anchor、target五个字符串字段。
题干明确具体年份、主体与指标，不出现答案；subject、indicator在题干和anchor内都逐字出现；
anchor必须是生成的材料中唯一的连续原文片段，内含subject、indicator和目标数字；
target为anchor中完整、唯一的数字文本（不带单位），五题目标位置不可重复。
自行核对材料、定位题与答案，标明它们属于模拟训练数据；不要输出解释或过程。"#,
            year=year,topic=source.topic,subtopic=source.subtopic,
            excerpt=generation_excerpt(source,0).text);
        let mut reason=String::new();
        for _ in 0..2 {
            let prompt=if reason.is_empty(){system.clone()}
                else{format!("上次无法匹配5个数字位置：{reason}。重新返回完整材料和五题，确保anchor和target逐字一致，仅JSON。")};
            let output=tokio::time::timeout(TURN_TIMEOUT,session.turn_locating(&prompt)).await
                .map_err(|_|"Codex 找数生成超时".to_owned())??;
            #[derive(Deserialize)] struct Batch{material_text:String,questions:Vec<LocatingRecipe>}
            match serde_json::from_str::<Batch>(&output){
                Ok(batch)=>{
                    let mut material=source.clone();
                    material.text=format!("【模拟训练数据，非官方发布数值】\n{}",batch.material_text);
                    match verify_locators(&material,&batch.questions){
                        Ok(targets)=>return Ok((material,targets)),
                        Err(e)=>reason=e
                    }
                }
                Err(_)=>reason="找数题JSON格式无效".into()
            }
        }
        Err(format!("找数答案位置无法匹配：{reason}"))
    }

    async fn locating_row(
        &self,
        id: &str,
    ) -> Result<
        (
            Material,
            Vec<LocatingTarget>,
            Vec<Option<LocatingAnswer>>,
            String,
        ),
        String,
    > {
        let row=sqlx::query("SELECT r.questions_json,r.answers_json,r.status,g.group_json FROM data_analysis_locating_runs r JOIN data_analysis_groups g ON g.id=r.group_id WHERE r.id=?")
            .bind(id).fetch_optional(&self.pool).await.map_err(as_error)?.ok_or("找数训练不存在")?;
        let group: QuestionGroup =
            serde_json::from_str(&row.try_get::<String, _>("group_json").map_err(as_error)?)
                .map_err(as_error)?;
        let questions = serde_json::from_str(
            &row.try_get::<String, _>("questions_json")
                .map_err(as_error)?,
        )
        .map_err(as_error)?;
        let answers =
            serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                .map_err(as_error)?;
        let status = row.try_get("status").map_err(as_error)?;
        Ok((group.material, questions, answers, status))
    }

    pub async fn locating_run(&self, id: &str) -> Result<LocatingView, String> {
        let (material, questions, answers, status) = self.locating_row(id).await?;
        if status == "ready" {
            self.mark_material_seen(&material, "locating").await?;
            sqlx::query("UPDATE data_analysis_locating_runs SET status='active',started_at_ms=? WHERE id=? AND status='ready'")
                .bind(now_ms()).bind(id).execute(&self.pool).await.map_err(as_error)?;
        }
        Ok(LocatingView {
            id: id.into(),
            status: if status == "ready" {
                "active".into()
            } else {
                status
            },
            material,
            questions: questions
                .into_iter()
                .map(|q| LocatingPublicQuestion {
                    question: q.question,
                })
                .collect(),
            answers,
        })
    }

    pub async fn locating_answer(
        &self,
        id: &str,
        number: usize,
        click: LocateClick,
    ) -> Result<LocatingView, String> {
        let (material, questions, mut answers, status) = self.locating_row(id).await?;
        if status != "active"
            || !(1..=LOCATING_COUNT).contains(&number)
            || questions.len() != LOCATING_COUNT
            || answers.len() != LOCATING_COUNT
        {
            return Err("找数训练状态或题号无效".into());
        }
        let index = number - 1;
        if answers[index].is_some() || answers.iter().take(index).any(Option::is_none) {
            return Err("只能按顺序完成尚未作答的题目".into());
        }
        let target = &questions[index];
        let correct = click.start == target.start
            && click.end == target.end
            && material.text.get(click.start..click.end).is_some();
        let answer = LocatingAnswer {
            correct,
            elapsed_ms: click.elapsed_ms.min(300_000),
            selected_start: click.start,
            expected_start: target.start,
        };
        let row = sqlx::query_scalar::<_, String>(
            "SELECT answers_json FROM data_analysis_locating_runs WHERE id=?",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(as_error)?;
        answers[index] = Some(answer);
        let status = if number == LOCATING_COUNT {
            "complete"
        } else {
            "active"
        };
        let result=sqlx::query("UPDATE data_analysis_locating_runs SET answers_json=?,status=? WHERE id=? AND status='active' AND answers_json=?")
            .bind(serde_json::to_string(&answers).map_err(as_error)?)
            .bind(status).bind(id).bind(row)
            .execute(&self.pool).await.map_err(as_error)?;
        if result.rows_affected() != 1 {
            return Err("提交冲突，请刷新训练状态".into());
        }
        Ok(LocatingView {
            id: id.into(),
            status: status.into(),
            material,
            questions: questions
                .into_iter()
                .map(|q| LocatingPublicQuestion {
                    question: q.question,
                })
                .collect(),
            answers,
        })
    }
}

fn matches_filter(material: &Material, request: &LocatingRequest) -> bool {
    request.topic.as_ref().is_none_or(|v| v == &material.topic)
        && request
            .subtopic
            .as_ref()
            .is_none_or(|v| v == &material.subtopic)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> Material {
        Material {
            source_url:"https://stats.gov.cn/test".into(),section:"三、工业和建筑业".into(),
            topic:"工业能源".into(),subtopic:"工业增加值".into(),
            text:"2024年，国有控股企业利润总额92亿元，同比增长6.4%；私营企业利润总额118亿元，同比增长9.7%。".into()
        }
    }
    fn recipe(anchor: &str, target: &str, subject: &str) -> LocatingRecipe {
        LocatingRecipe {
            question: format!("2024年{subject}利润总额对应的数字是多少？"),
            subject: subject.into(),
            indicator: "利润总额".into(),
            anchor: anchor.into(),
            target: target.into(),
        }
    }
    #[test]
    fn precise_location_rejects_same_number_inside_another_subject() {
        let m = material();
        let q = recipe("国有控股企业利润总额92亿元", "92", "国有控股企业");
        let mut recipes = vec![q.clone(); 5];
        recipes[1] = recipe("私营企业利润总额118亿元", "118", "私营企业");
        assert!(verify_locators(&m, &recipes).is_err());
    }
    #[test]
    fn rejects_numeric_substring_and_ambiguous_anchor() {
        let m = material();
        let q = recipe("私营企业利润总额118亿元", "18", "私营企业");
        assert!(verify_locators(&m, &vec![q; 5]).is_err());
    }
    #[test]
    fn previously_reworded_same_chapter_is_same_material() {
        let mut m = material();
        let key = material_key(&m);
        m.text = "换一篇同样的统计章节".into();
        assert_eq!(material_key(&m), key);
    }
    #[test]
    fn simulated_papers_with_same_official_reference_are_distinct() {
        let mut m = material();
        m.text = "【模拟训练数据，非官方发布数值】\n2024年企业收入100万元。".into();
        let first = material_key(&m);
        m.text = "【模拟训练数据，非官方发布数值】\n2024年企业收入200万元。".into();
        assert_ne!(material_key(&m), first);
        let repeated = material_key(&m);
        assert_eq!(repeated, material_key(&m));
    }
}
