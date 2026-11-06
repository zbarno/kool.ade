use super::super::ModelCallUsage;

pub(super) fn merge_model_call(mut next: ModelCallUsage, prior: &ModelCallUsage) -> ModelCallUsage {
    next.provider = next.provider.or_else(|| prior.provider.clone());
    next.api = next.api.or_else(|| prior.api.clone());
    next.model = next.model.or_else(|| prior.model.clone());
    next.requested_model = next
        .requested_model
        .or_else(|| prior.requested_model.clone());
    next.input_tokens = next.input_tokens.or(prior.input_tokens);
    next.output_tokens = next.output_tokens.or(prior.output_tokens);
    next.cache_read_tokens = next.cache_read_tokens.or(prior.cache_read_tokens);
    next.cache_write_tokens = next.cache_write_tokens.or(prior.cache_write_tokens);
    next.reasoning_tokens = next.reasoning_tokens.or(prior.reasoning_tokens);
    next.total_tokens = next.total_tokens.or(prior.total_tokens);
    next.estimated_cost_usd_micros = next
        .estimated_cost_usd_micros
        .or(prior.estimated_cost_usd_micros);
    next.started_at = next.started_at.or(prior.started_at);
    next.ended_at = next.ended_at.or(prior.ended_at);
    next.duration_millis = next.duration_millis.or(prior.duration_millis);
    next.stop_reason = next.stop_reason.or_else(|| prior.stop_reason.clone());
    next
}
