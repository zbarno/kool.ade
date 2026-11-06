use super::{
    LiveProgress, TaskActivityActor, TaskActivityEventV1, TaskActivityOrigin,
    TaskActivityVisibility, bounded_activity_text, safe_activity_text,
};
mod bounds;
use bounds::bound_posts;
mod usage;

#[cfg(test)]
#[path = "stream/tests.rs"]
mod tests;

impl LiveProgress {
    /// Start a new isolated task activity run and retain its visible boundary.
    pub fn begin_task_run(&mut self, task_uid: impl Into<String>) {
        self.task_uid = Some(task_uid.into());
        self.run_id = Some(uuid::Uuid::new_v4().to_string());
        self.run_attempt = self
            .activity_events
            .iter()
            .map(|event| event.attempt)
            .max()
            .unwrap_or(self.run_attempt)
            .saturating_add(1);
        self.append_activity_status("run", "started", "Worker run started");
    }

    /// Record application-owned lifecycle events in the same local stream.
    pub fn append_activity_status(&mut self, kind: &str, status: &str, content: &str) {
        let (Some(task_uid), Some(run_id)) = (&self.task_uid, &self.run_id) else {
            return;
        };
        let recorded_at_ms = chrono::Utc::now().timestamp_millis();
        let event = TaskActivityEventV1 {
            schema_version: 1,
            event_id: format!(
                "{run_id}:app:{recorded_at_ms}:{}",
                self.activity_events.len()
            ),
            sequence: 0,
            task_uid: task_uid.clone(),
            run_id: run_id.clone(),
            attempt: self.run_attempt,
            provider: self
                .selected_route
                .as_deref()
                .map(super::safe_activity_text),
            recorded_at_ms,
            actor: TaskActivityActor::System,
            origin: TaskActivityOrigin::ApplicationInferred,
            kind: kind.into(),
            status: Some(status.into()),
            content: bounded_activity_text(content, 2_000),
            visibility: TaskActivityVisibility::UserSafe,
        };
        upsert_event(&mut self.activity_events, event);
        self.bound_activity_events();
    }

    pub(super) fn bound_activity_events(&mut self) {
        const MAX_ACTIVITY_EVENTS: usize = 500;
        if self.activity_events.len() > MAX_ACTIVITY_EVENTS {
            let remove = self.activity_events.len() - MAX_ACTIVITY_EVENTS;
            self.activity_events_truncated_before_ms = self
                .activity_events
                .get(remove - 1)
                .map(|event| event.recorded_at_ms);
            self.activity_events.drain(..remove);
        }
    }
}

pub(super) fn actor_for_kind(kind: &str) -> TaskActivityActor {
    let kind = kind.to_ascii_lowercase();
    if kind.contains("review") || kind.contains("qa") {
        TaskActivityActor::Reviewer
    } else if kind.contains("status") || kind.contains("verification") {
        TaskActivityActor::System
    } else {
        TaskActivityActor::Worker
    }
}

impl LiveProgress {
    pub fn update(&mut self, mut next: Self) {
        if next.prompt_manifest.is_none() {
            next.prompt_manifest = self.prompt_manifest.clone();
        }
        // Progress snapshots can be delayed behind a newer snapshot. Checklist
        // reports carry a worker-local revision so late snapshots cannot roll
        // the durable board state back.
        if next.checklist_revision < self.checklist_revision {
            next.checklist = std::mem::take(&mut self.checklist);
            next.checklist_revision = self.checklist_revision;
        }
        for call in next.model_calls.drain(..) {
            if let Some(existing) = self
                .model_calls
                .iter_mut()
                .find(|item| item.call_id == call.call_id)
            {
                *existing = usage::merge_model_call(call, existing);
            } else {
                self.model_calls.push(call);
            }
        }
        next.model_calls = std::mem::take(&mut self.model_calls);
        for request in next.dependency_requests.drain(..) {
            if let Some(existing) = self
                .dependency_requests
                .iter_mut()
                .find(|item| item.id == request.id)
            {
                *existing = request;
            } else {
                self.dependency_requests.push(request);
            }
        }
        next.dependency_requests = std::mem::take(&mut self.dependency_requests);
        let observed_at_ms = chrono::Utc::now().timestamp_millis();
        let task_uid = next.task_uid.clone().or_else(|| self.task_uid.clone());
        let run_id = next.run_id.clone().or_else(|| self.run_id.clone());
        let attempt = next.run_attempt.max(self.run_attempt);
        for mut post in next.posts.drain(..) {
            post.text = bounded_activity_text(&post.text, 2_000);
            if post.kind.eq_ignore_ascii_case("thinking") {
                continue;
            }
            post.recorded_at_ms.get_or_insert(observed_at_ms);
            post.run_id = run_id.clone().or(post.run_id);
            let existing_index = self
                .posts
                .iter()
                .position(|p| p.id == post.id && p.run_id == post.run_id);
            if let Some(index) = existing_index {
                post.recorded_at_ms = self.posts[index].recorded_at_ms.or(post.recorded_at_ms);
            }
            if let (Some(task_uid), Some(run_id)) = (task_uid.as_deref(), run_id.as_deref()) {
                let event_id = format!("{run_id}:{}:{}", post.id.0, post.id.1);
                let event = TaskActivityEventV1 {
                    schema_version: 1,
                    event_id,
                    sequence: 0,
                    task_uid: task_uid.to_owned(),
                    run_id: run_id.to_owned(),
                    attempt,
                    provider: next
                        .selected_route
                        .as_deref()
                        .or(self.selected_route.as_deref())
                        .map(safe_activity_text),
                    recorded_at_ms: post.recorded_at_ms.unwrap_or(observed_at_ms),
                    actor: actor_for_kind(&post.kind),
                    origin: TaskActivityOrigin::ProviderObserved,
                    kind: post.kind.clone(),
                    status: None,
                    content: post.text.clone(),
                    visibility: TaskActivityVisibility::UserSafe,
                };
                upsert_event(&mut self.activity_events, event);
            }
            if let Some(index) = existing_index {
                self.posts[index] = post;
            } else {
                self.posts.push(post);
            }
        }
        bound_posts(&mut self.posts);
        if !next.response.trim().is_empty()
            && let (Some(task_uid), Some(run_id)) = (task_uid.as_deref(), run_id.as_deref())
        {
            let event = TaskActivityEventV1 {
                schema_version: 1,
                event_id: format!("{run_id}:response"),
                sequence: 0,
                task_uid: task_uid.to_owned(),
                run_id: run_id.to_owned(),
                attempt,
                provider: next
                    .selected_route
                    .as_deref()
                    .or(self.selected_route.as_deref())
                    .map(safe_activity_text),
                recorded_at_ms: observed_at_ms,
                actor: TaskActivityActor::Worker,
                origin: TaskActivityOrigin::ProviderObserved,
                kind: "response".into(),
                status: None,
                content: bounded_activity_text(&next.response, 32_000),
                visibility: TaskActivityVisibility::UserSafe,
            };
            upsert_event(&mut self.activity_events, event);
        }
        if let Some(activity) = next
            .activity
            .as_deref()
            .filter(|value| !value.trim().is_empty() && self.activity.as_deref() != Some(*value))
            && let (Some(task_uid), Some(run_id)) = (task_uid.as_deref(), run_id.as_deref())
        {
            let event = TaskActivityEventV1 {
                schema_version: 1,
                event_id: format!(
                    "{run_id}:status:{observed_at_ms}:{}",
                    self.activity_events.len()
                ),
                sequence: 0,
                task_uid: task_uid.to_owned(),
                run_id: run_id.to_owned(),
                attempt,
                provider: next
                    .selected_route
                    .as_deref()
                    .or(self.selected_route.as_deref())
                    .map(safe_activity_text),
                recorded_at_ms: observed_at_ms,
                actor: TaskActivityActor::System,
                origin: TaskActivityOrigin::ProviderObserved,
                kind: "status".into(),
                status: None,
                content: bounded_activity_text(activity, 2_000),
                visibility: TaskActivityVisibility::UserSafe,
            };
            upsert_event(&mut self.activity_events, event);
        }
        self.activity_events.sort_by(|a, b| {
            a.recorded_at_ms
                .cmp(&b.recorded_at_ms)
                .then_with(|| a.sequence.cmp(&b.sequence))
        });
        self.bound_activity_events();
        next.posts = std::mem::take(&mut self.posts);
        next.activity_events = std::mem::take(&mut self.activity_events);
        next.activity_events_truncated_before_ms = self
            .activity_events_truncated_before_ms
            .or(next.activity_events_truncated_before_ms);
        let now = chrono::Utc::now().timestamp_millis();
        let mut telemetry = std::mem::take(&mut self.telemetry);
        telemetry.started_ms.get_or_insert(now);
        telemetry.updated_ms = Some(now);
        telemetry.updates += 1;
        let bucket = now / 10_000;
        if let Some((_, count)) = telemetry
            .samples
            .last_mut()
            .filter(|(last, _)| *last == bucket)
        {
            *count += 1;
        } else {
            telemetry.samples.push((bucket, 1));
        }
        telemetry.samples.retain(|(time, _)| *time >= bucket - 59);
        telemetry.launch_profile = telemetry
            .launch_profile
            .or_else(|| next.telemetry.launch_profile.clone());
        next.telemetry = telemetry;
        next.selected_route = next.selected_route.or_else(|| self.selected_route.clone());
        next.task_uid = next.task_uid.or_else(|| self.task_uid.clone());
        next.run_id = next.run_id.or_else(|| self.run_id.clone());
        next.run_attempt = next.run_attempt.max(self.run_attempt);
        next.thoughts.clear();
        next.response = bounded_activity_text(&next.response, 32_000);
        next.specification = next
            .specification
            .map(|text| bounded_activity_text(&text, 32_000));
        next.activity = next
            .activity
            .map(|text| bounded_activity_text(&text, 2_000));
        *self = next;
    }
}

fn upsert_event(events: &mut Vec<TaskActivityEventV1>, mut event: TaskActivityEventV1) {
    if let Some(existing) = events
        .iter_mut()
        .find(|existing| existing.event_id == event.event_id)
    {
        event.recorded_at_ms = existing.recorded_at_ms;
        event.sequence = existing.sequence;
        *existing = event;
    } else {
        event.sequence = events
            .iter()
            .map(|existing| existing.sequence)
            .max()
            .unwrap_or_default()
            .saturating_add(1);
        events.push(event);
    }
}
