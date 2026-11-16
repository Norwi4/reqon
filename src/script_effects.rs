use crate::collection_runner::apply_values as apply;
use crate::{app::App, model::*};
impl App {
    pub fn apply_script_effects(
        &mut self,
        cid: &str,
        environment: &str,
        workspace_env: &str,
        effects: &ScriptEffects,
    ) {
        let globals = if let Some(env) = self
            .ws
            .environments
            .iter_mut()
            .find(|e| e.name == workspace_env)
        {
            &mut env.vars
        } else {
            &mut self.ws.globals
        };
        apply(globals, &effects.globals, &effects.deleted_globals);
        if let Some(collection) = self.ws.collections.iter_mut().find(|c| c.id == cid) {
            apply(&mut collection.vars, &effects.collection, &[]);
            if let Some(env) = collection
                .environments
                .iter_mut()
                .find(|e| e.name == environment)
            {
                apply(
                    &mut env.vars,
                    &effects.environment,
                    &effects.deleted_environment,
                );
            }
            if effects.persist_environment || !effects.collection.is_empty() {
                self.writer.save_environments(collection.clone());
            }
        }
    }
}
