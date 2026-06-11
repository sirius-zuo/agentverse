use agentverse::{Tool, ToolError, ToolResult};
use chrono::{Duration, NaiveDate};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Phase {
    /// Phase name (unique within this project)
    pub name: String,
    /// Phase duration in weeks
    pub duration_weeks: u32,
    /// Names of phases this phase depends on (must finish before this one starts)
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MilestoneArgs {
    /// Project start date in YYYY-MM-DD format
    pub start_date: String,
    pub phases: Vec<Phase>,
}

pub struct MilestoneScheduler;

#[async_trait::async_trait]
impl Tool for MilestoneScheduler {
    type Args = MilestoneArgs;
    fn name(&self) -> &str { "milestone_scheduler" }
    fn description(&self) -> &str {
        "Schedule project phases from a start date, respecting phase dependencies. \
         Returns per-phase start/end dates, total project duration in weeks and months."
    }
    async fn execute(&self, args: MilestoneArgs) -> ToolResult {
        let start = NaiveDate::parse_from_str(&args.start_date, "%Y-%m-%d").map_err(|_| {
            ToolError::Execution(format!(
                "Invalid date '{}': expected YYYY-MM-DD",
                args.start_date
            ))
        })?;

        let mut end_by_name: HashMap<String, NaiveDate> = HashMap::new();
        let mut schedule = Vec::new();

        for phase in &args.phases {
            for dep in &phase.depends_on {
                if !end_by_name.contains_key(dep.as_str()) {
                    return Err(ToolError::Execution(format!(
                        "Phase '{}' depends on '{}', which has not been scheduled yet — \
                         list phases in dependency order",
                        phase.name, dep
                    )));
                }
            }

            let phase_start = phase
                .depends_on
                .iter()
                .filter_map(|dep| end_by_name.get(dep))
                .max()
                .copied()
                .unwrap_or(start);

            let phase_end = phase_start + Duration::weeks(phase.duration_weeks as i64);
            end_by_name.insert(phase.name.clone(), phase_end);

            schedule.push(json!({
                "name":           phase.name,
                "start":          phase_start.format("%Y-%m-%d").to_string(),
                "end":            phase_end.format("%Y-%m-%d").to_string(),
                "duration_weeks": phase.duration_weeks,
            }));
        }

        let project_end = end_by_name.values().max().copied().unwrap_or(start);
        let total_weeks = (project_end - start).num_weeks();

        Ok(json!({
            "phases":                    schedule,
            "project_end_date":          project_end.format("%Y-%m-%d").to_string(),
            "total_duration_weeks":      total_weeks,
            "total_duration_months":     ((total_weeks as f64 / 4.33).round() as i64),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn milestone_scheduler_sequential_phases() {
        let tool = MilestoneScheduler;
        let args = MilestoneArgs {
            start_date: "2026-01-01".into(),
            phases: vec![
                Phase { name: "Discovery".into(), duration_weeks: 4, depends_on: vec![] },
                Phase {
                    name: "MVP".into(),
                    duration_weeks: 8,
                    depends_on: vec!["Discovery".into()],
                },
            ],
        };
        let result = tool.execute(args).await.unwrap();
        let phases = result["phases"].as_array().unwrap();
        assert_eq!(phases[0]["start"], "2026-01-01");
        assert_eq!(phases[0]["end"], "2026-01-29");   // +28 days
        assert_eq!(phases[1]["start"], "2026-01-29");
        assert_eq!(phases[1]["end"], "2026-03-26");   // +56 days
        assert_eq!(result["total_duration_weeks"], 12);
    }

    #[tokio::test]
    async fn milestone_scheduler_rejects_invalid_date() {
        let tool = MilestoneScheduler;
        let args = MilestoneArgs {
            start_date: "not-a-date".into(),
            phases: vec![],
        };
        assert!(tool.execute(args).await.is_err());
    }

    #[tokio::test]
    async fn milestone_scheduler_rejects_unknown_dependency() {
        // "MVP" lists "Discovery" as a dependency, but "Discovery" is not in the phase list.
        // The tool must return an error rather than silently starting MVP at the project start.
        let tool = MilestoneScheduler;
        let args = MilestoneArgs {
            start_date: "2026-01-01".into(),
            phases: vec![Phase {
                name: "MVP".into(),
                duration_weeks: 8,
                depends_on: vec!["Discovery".into()],
            }],
        };
        assert!(tool.execute(args).await.is_err());
    }
}
