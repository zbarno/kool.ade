use crate::execution::contract::ResourceBudgetV1;
use std::{collections::BTreeMap, sync::Mutex};

static RESOURCE_BUDGETS: Mutex<BTreeMap<String, ResourceBudgetV1>> = Mutex::new(BTreeMap::new());

pub(crate) struct TestResourceBudgetOverrides(Vec<(String, Option<ResourceBudgetV1>)>);

pub(crate) fn override_test_resource_budgets(
    overrides: impl IntoIterator<Item = (String, ResourceBudgetV1)>,
) -> TestResourceBudgetOverrides {
    let mut budgets = RESOURCE_BUDGETS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut previous = BTreeMap::new();
    for (task_id, budget) in overrides {
        previous
            .entry(task_id.clone())
            .or_insert_with(|| budgets.get(&task_id).cloned());
        budgets.insert(task_id, budget);
    }
    TestResourceBudgetOverrides(previous.into_iter().collect())
}

impl Drop for TestResourceBudgetOverrides {
    fn drop(&mut self) {
        let mut budgets = RESOURCE_BUDGETS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (task_id, previous) in self.0.drain(..) {
            if let Some(budget) = previous {
                budgets.insert(task_id, budget);
            } else {
                budgets.remove(&task_id);
            }
        }
    }
}

pub(crate) fn resource_budget_override(task_id: &str) -> Option<ResourceBudgetV1> {
    RESOURCE_BUDGETS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(task_id)
        .cloned()
}
