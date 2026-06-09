// Stub — implemented in Task 3
use crate::types::{Skill, SkillContext, SkillId};

pub struct SkillRegistry;

impl SkillRegistry {
    pub fn load(_dir: &std::path::Path) -> std::sync::Arc<Self> {
        unimplemented!("stub")
    }
    pub fn get(&self, _id: &str) -> Option<&Skill> {
        None
    }
    pub fn compile_context(&self, _id: &str) -> Result<SkillContext, crate::error::SkillError> {
        unimplemented!("stub")
    }
}
