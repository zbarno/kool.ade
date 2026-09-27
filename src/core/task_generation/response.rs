use crate::{
    core::workflow::{TaskOutline, TaskStory, story_detail_errors},
    harness::responses::{self, TaskOutlineResponse, TaskStoryResponse},
};

const MAX_STORY_CHARS: usize = 8_000;
const STORY_FIELD_BUDGETS: &[(&str, usize)] = &[
    ("intent", 600),
    ("goal", 600),
    ("context", 900),
    ("user story", 400),
    ("purpose", 500),
    ("affected files", 800),
    ("implementation steps", 1_900),
    ("acceptance criteria", 900),
    ("test plan", 850),
    ("verification expectations", 400),
    ("definition of done", 320),
    ("technical design", 500),
    ("edge cases", 600),
    ("rollout notes", 200),
];

pub(super) fn decode_outline(text: &str) -> Result<Vec<TaskOutline>, Vec<String>> {
    let response = responses::decode::<TaskOutlineResponse>(text).map_err(|error| {
        vec![format!(
            "Invalid task outline schema: {error}. Return one bare JSON object, without prose or a code fence. task_outline must be an array; scope_items and success_criteria must be arrays of one-based integer indexes into the approved brief; dependencies must be an array of earlier outline positions. Never put descriptions in these arrays; put explanation in purpose. Return an empty or null task_stories field."
        )]
    })?;
    responses::normalize_task_outline(response).map_err(|error| vec![error])
}
pub(super) fn decode_stories(text: &str) -> Result<Vec<TaskStory>, Vec<String>> {
    let mut response = responses::decode::<serde_json::Value>(text).map_err(|error| {
        let repair = if error.contains("no complete JSON object") {
            "No complete JSON object was found. Return ONLY one complete JSON object with exactly one task_stories entry. No preface, explanation, Markdown fence, or second attempt. Keep it under 3,000 characters, use short plain-text values, and do not include code snippets or quoted phrases inside strings. The first non-whitespace character must be { and the last must be }. Apostrophes are ordinary characters; do not escape them."
        } else {
            "The JSON object is malformed. Discard it and rebuild a minimal complete object under 3,000 characters. Return ONLY one object with exactly one task_stories entry: no preface, explanation, Markdown fence, or second attempt. Use short plain-text strings with no code snippets, quoted phrases, or backslashes inside values. Every list field is one flat array of strings. Apostrophes are ordinary characters and must not be escaped. Escape any required JSON quotes and control characters correctly."
        };
        vec![format!(
            "Invalid task story JSON: {error}. {repair} Return one complete JSON object with exactly one task_stories entry and the supplied fields. All story list fields must be JSON arrays, including when they contain one item. Do not add a preface, commentary, or code fence."
        )]
    })?;
    normalize_story_list_fields(&mut response);
    let response = serde_json::from_value::<TaskStoryResponse>(response).map_err(|error| {
        let repair = if error.to_string().contains("invalid type: sequence, expected a string") {
            "A string field received an array, or a list field contains a nested array. Keep intent, goal, context, user_story, purpose, and rollout_notes as strings. Use arrays of strings for every list field; never nest arrays or objects."
        } else {
            "The root must be the response envelope, not a story: {\"task_stories\":[{\"title\":\"...\",\"acceptance_criteria\":[\"...\"]}]}. Put every story field inside the single task_stories array entry and keep the complete object under 4,000 characters."
        };
        vec![format!(
            "Invalid task story schema: {error}. {repair} Keep one task_stories entry and all story fields in their declared types."
        )]
    })?;
    responses::normalize_task_story(response).map_err(|error| vec![error])
}

fn normalize_story_list_fields(response: &mut serde_json::Value) {
    let key = if response.get("taskStories").is_some() {
        "taskStories"
    } else {
        "task_stories"
    };
    let stories = response.get_mut(key);
    let Some(stories) = stories.and_then(serde_json::Value::as_array_mut) else {
        return;
    };
    const LIST_FIELDS: &[&str] = &[
        "affectedFiles",
        "affected_files",
        "implementationSteps",
        "implementation_steps",
        "acceptanceCriteria",
        "acceptance_criteria",
        "testPlan",
        "test_plan",
        "verificationCommands",
        "verification_commands",
        "technicalDesign",
        "technical_design",
        "edgeCases",
        "edge_cases",
        "definitionOfDone",
        "definition_of_done",
    ];
    for story in stories
        .iter_mut()
        .filter_map(serde_json::Value::as_object_mut)
    {
        for field in LIST_FIELDS {
            if story.get(*field).is_some_and(serde_json::Value::is_string) {
                let value = story.get_mut(*field).expect("field checked above").take();
                story.insert((*field).into(), serde_json::Value::Array(vec![value]));
            }
        }
    }
}
pub(super) fn same_refs(a: &[usize], b: &[usize]) -> bool {
    a.iter().copied().collect::<std::collections::BTreeSet<_>>() == b.iter().copied().collect()
}
/// The app owns stable identifiers and wording from the accepted outline.
/// Omitted references are filled in; actual scope/dependency changes are repaired.
pub(super) fn story_response(
    text: &str,
    planned: &TaskOutline,
    index: usize,
) -> Result<TaskStory, Vec<String>> {
    let mut stories = decode_stories(text)?;
    if stories.len() != 1 {
        return Err(vec![
            "Return exactly one complete story in task_stories.".into(),
        ]);
    }
    let mut story = stories.remove(0);
    if !story.target_repository.is_empty() && story.target_repository != planned.target_repository {
        return Err(vec![
            "target_repository changed the approved outline repository".into(),
        ]);
    }
    for (name, values, expected) in [
        ("scope_items", &story.scope_items, &planned.scope_items),
        (
            "success_criteria",
            &story.success_criteria,
            &planned.success_criteria,
        ),
        ("dependencies", &story.dependencies, &planned.dependencies),
    ] {
        if !values.is_empty() && !same_refs(values, expected) {
            return Err(vec![format!(
                "{name} changed the task's approved mapping. Expected {expected:?}; received {values:?}."
            )]);
        }
    }
    story.title = planned.title.clone();
    story.purpose = planned.purpose.clone();
    story.target_repository = planned.target_repository.clone();
    story.scope_items = planned.scope_items.clone();
    story.success_criteria = planned.success_criteria.clone();
    story.dependencies = planned.dependencies.clone();
    let serialized_chars = serde_json::to_string(&story)
        .map_err(|error| vec![format!("Could not measure task story JSON: {error}")])?
        .chars()
        .count();
    let lengths = story_field_lengths(&story);
    let budget_overages = lengths
        .iter()
        .filter_map(|(name, length)| {
            STORY_FIELD_BUDGETS
                .iter()
                .find(|(field, _)| field == name)
                .filter(|(_, budget)| length > budget)
                .map(|(_, budget)| format!("{name} {length}/{budget}"))
        })
        .collect::<Vec<_>>();
    if serialized_chars > MAX_STORY_CHARS || !budget_overages.is_empty() {
        let total_trim = if serialized_chars > MAX_STORY_CHARS {
            format!(
                "Remove at least {} serialized characters overall to meet the hard limit; target 4,000 characters or fewer for reliable repair.",
                serialized_chars - MAX_STORY_CHARS
            )
        } else {
            "The story is within the total serialized-size limit.".into()
        };
        return Err(vec![format!(
            "Task {}: the complete story is {serialized_chars} characters; the limit is {MAX_STORY_CHARS}. {total_trim} Field budget overages: {}. Longest fields: {}. For an oversized retry, cut whole redundant sentences and optional detail rather than making a minimal edit. Trim over-budget fields first, avoid restating behavior across fields, and leave optional detail empty when other fields cover it.",
            index + 1,
            if budget_overages.is_empty() {
                "none".to_owned()
            } else {
                budget_overages.join(", ")
            },
            longest_story_fields(&lengths)
        )]);
    }
    let errors = story_detail_errors(&story, index + 1);
    if errors.is_empty() {
        Ok(story)
    } else {
        Err(errors)
    }
}

fn story_field_lengths(story: &TaskStory) -> Vec<(&'static str, usize)> {
    vec![
        ("intent", story.intent.chars().count()),
        ("goal", story.goal.chars().count()),
        ("context", story.context.chars().count()),
        ("user story", story.user_story.chars().count()),
        ("purpose", story.purpose.chars().count()),
        ("affected files", entry_chars(&story.affected_files)),
        (
            "implementation steps",
            entry_chars(&story.implementation_steps),
        ),
        (
            "acceptance criteria",
            entry_chars(&story.acceptance_criteria),
        ),
        ("test plan", entry_chars(&story.test_plan)),
        (
            "verification expectations",
            entry_chars(&story.verification_commands),
        ),
        ("definition of done", entry_chars(&story.definition_of_done)),
        ("technical design", entry_chars(&story.technical_design)),
        ("edge cases", entry_chars(&story.edge_cases)),
        ("rollout notes", story.rollout_notes.chars().count()),
    ]
}

fn longest_story_fields(lengths: &[(&str, usize)]) -> String {
    let mut lengths = lengths.to_vec();
    lengths.sort_by_key(|(_, length)| std::cmp::Reverse(*length));
    lengths
        .into_iter()
        .take(5)
        .map(|(name, length)| format!("{name} ({length} characters)"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn entry_chars(entries: &[String]) -> usize {
    entries.iter().map(|entry| entry.chars().count()).sum()
}

pub(super) fn specification_h1_feature_id(spec: &str) -> Option<String> {
    spec.lines()
        .find(|line| line.starts_with('#'))
        .and_then(|line| {
            crate::core::workflow::feature_ids_in(line)
                .into_iter()
                .next()
        })
}
