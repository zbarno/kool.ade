use super::super::KooladeApp;

impl KooladeApp {
    pub(crate) fn approve_and_prepare_feature(&mut self, id: &str) {
        self.approve_feature_action(id, true);
    }

    pub(crate) fn approve_feature_only(&mut self, id: &str) {
        self.approve_feature_action(id, false);
    }
}
