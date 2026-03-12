use super::*;

impl RuntimeHandle {
    pub fn register_tool<T>(&self, tool: T)
    where
        T: ExecutableTool + 'static,
    {
        self.tool_registry
            .write()
            .expect("tool registry poisoned")
            .register_tool(tool);
    }

    pub fn register_skill_loader(&self, loader: SkillLoader) {
        *self.skill_loader.write().expect("skill loader poisoned") = Some(loader);
        self.tool_registry
            .write()
            .expect("tool registry poisoned")
            .register_tool(crate::tool::builtin::LoadSkillTool);
    }

    pub fn tools(&self) -> Arc<[ToolSpec]> {
        self.tool_registry
            .read()
            .expect("tool registry poisoned")
            .tools()
    }

    pub fn store(&self) -> Arc<dyn RuntimeStore> {
        self.store.clone()
    }

    pub fn skill_descriptions(&self) -> Option<String> {
        self.skill_loader
            .read()
            .expect("skill loader poisoned")
            .as_ref()
            .map(SkillLoader::get_descriptions)
            .filter(|descriptions| !descriptions.is_empty())
    }

    pub fn load_skill(&self, name: &str) -> Result<String, String> {
        let skills = self.skill_loader.read().expect("skill loader poisoned");
        let Some(loader) = skills.as_ref() else {
            return Err("Skill loader is not available".to_string());
        };

        loader.get_content(name)
    }

    pub fn get_tool(&self, name: &str) -> Option<Arc<dyn ExecutableTool>> {
        self.tool_registry
            .read()
            .expect("tool registry poisoned")
            .get_tool(name)
    }
}
