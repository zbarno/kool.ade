use super::*;

impl KooladeApp {
    pub(in crate::app::root::surface) fn activity_samples_for_surface(
        &self,
        key: Option<&str>,
    ) -> Vec<(i64, u64)> {
        let Screen::Connected(project) = &self.screen else {
            return Vec::new();
        };
        if key.is_none()
            && let Some(overall) = &project.activity.overall
        {
            return overall.telemetry.samples.clone();
        }
        let item_id = key
            .and_then(|key| {
                project
                    .state
                    .items
                    .iter()
                    .chain(&project.state.resolved_items)
                    .find(|item| item.conversation_key() == key)
                    .map(|item| item.id.as_str())
            })
            .or(key);
        let mut buckets = std::collections::BTreeMap::<i64, u64>::new();
        for (id, progress) in &project.activity.tasks {
            if item_id.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        for (id, progress) in &project.activity.conversations {
            if key.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        buckets.into_iter().collect()
    }
}
