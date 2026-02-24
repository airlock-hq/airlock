//! Workflow configuration types.
//!
//! Each `.airlock/workflows/*.yml` file deserializes into a `WorkflowConfig`.
//! This follows GitHub Actions syntax conventions: workflows contain jobs,
//! jobs contain steps, and trigger filters control which branches activate a workflow.

use std::collections::{HashMap, HashSet, VecDeque};

use glob_match::glob_match;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::types::StepDefinition;

/// A single workflow configuration, loaded from one `.airlock/workflows/*.yml` file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowConfig {
    /// Optional display name for the workflow.
    #[serde(default)]
    pub name: Option<String>,

    /// Trigger filter. If the pushed branch doesn't match, this workflow doesn't run.
    #[serde(default)]
    pub on: Option<TriggerConfig>,

    /// Named jobs (pipelines) to execute. Order is preserved via `IndexMap`.
    #[serde(default)]
    pub jobs: IndexMap<String, JobConfig>,
}

/// Trigger configuration for a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Push trigger with branch filters.
    #[serde(default)]
    pub push: Option<PushTrigger>,
}

/// Push trigger with branch inclusion/exclusion filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushTrigger {
    /// Branch patterns to match. Supports glob patterns (`*`, `**`).
    /// If empty or not specified, matches all branches.
    #[serde(default)]
    pub branches: Vec<String>,

    /// Branch patterns to exclude.
    #[serde(default, rename = "branches-ignore")]
    pub branches_ignore: Vec<String>,
}

/// Configuration for a single job within a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    /// Display name for the job.
    #[serde(default)]
    pub name: Option<String>,

    /// Jobs that must complete before this one starts.
    #[serde(default)]
    pub needs: OneOrMany,

    /// Ordered list of steps to execute within this job.
    #[serde(default)]
    pub steps: Vec<StepDefinition>,

    /// Keep worktree after job completion (debug flag).
    #[serde(default, rename = "keep-worktrees")]
    pub keep_worktrees: bool,
}

/// A helper type that deserializes from either a single string or a list of strings.
/// This matches GitHub Actions behavior where `needs` can be either:
/// - `needs: lint` (single string)
/// - `needs: [lint, test]` (array)
#[derive(Debug, Clone, Default, Serialize)]
pub struct OneOrMany(pub Vec<String>);

impl std::ops::Deref for OneOrMany {
    type Target = Vec<String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for OneOrMany {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct OneOrManyVisitor;

        impl<'de> de::Visitor<'de> for OneOrManyVisitor {
            type Value = OneOrMany;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a list of strings")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(OneOrMany(vec![v.to_string()]))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut vec = Vec::new();
                while let Some(val) = seq.next_element::<String>()? {
                    vec.push(val);
                }
                Ok(OneOrMany(vec))
            }
        }

        deserializer.deserialize_any(OneOrManyVisitor)
    }
}

// =============================================================================
// Branch matching
// =============================================================================

/// Check if a branch matches a workflow's trigger configuration.
///
/// - No trigger config (`None`) → matches all branches.
/// - No push trigger → matches all branches.
/// - Empty `branches` list → matches all branches.
/// - Non-empty `branches` → branch must match at least one pattern.
/// - `branches-ignore` → branch must NOT match any exclusion pattern.
pub fn branch_matches_trigger(branch: &str, trigger: &Option<TriggerConfig>) -> bool {
    match trigger {
        None => true,
        Some(config) => match &config.push {
            None => true,
            Some(push) => {
                let included = if push.branches.is_empty() {
                    true
                } else {
                    push.branches.iter().any(|p| glob_match(p, branch))
                };
                let excluded = push.branches_ignore.iter().any(|p| glob_match(p, branch));
                included && !excluded
            }
        },
    }
}

// =============================================================================
// DAG validation
// =============================================================================

/// Validate the job dependency graph and return execution waves.
///
/// Each wave is a group of jobs that can execute in parallel (all dependencies satisfied).
/// Returns an error if:
/// - A `needs` reference points to a non-existent job.
/// - The graph contains a cycle.
///
/// The returned waves are in topological order: wave 0 has no dependencies,
/// wave 1 depends only on wave 0, etc.
pub fn validate_job_dag(
    jobs: &IndexMap<String, JobConfig>,
) -> Result<Vec<Vec<String>>, DagValidationError> {
    let job_keys: HashSet<&str> = jobs.keys().map(|s| s.as_str()).collect();

    // Validate all needs references exist
    for (key, job) in jobs {
        for dep in job.needs.iter() {
            if !job_keys.contains(dep.as_str()) {
                return Err(DagValidationError::UnknownJob {
                    job: key.clone(),
                    unknown_dep: dep.clone(),
                });
            }
        }
    }

    // Build in-degree map and adjacency list
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for key in jobs.keys() {
        in_degree.insert(key.as_str(), 0);
        dependents.entry(key.as_str()).or_default();
    }

    for (key, job) in jobs {
        in_degree.insert(key.as_str(), job.needs.len());
        for dep in job.needs.iter() {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(key.as_str());
        }
    }

    // Kahn's algorithm: BFS topological sort, collecting waves
    let mut waves: Vec<Vec<String>> = Vec::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    let mut processed = 0usize;

    // Seed with zero in-degree nodes
    for (key, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(key);
        }
    }

    while !queue.is_empty() {
        // Drain current wave
        let wave: Vec<String> = queue.drain(..).map(|s| s.to_string()).collect();
        let mut next_queue = Vec::new();

        for job_key in &wave {
            processed += 1;
            if let Some(deps) = dependents.get(job_key.as_str()) {
                for &dep in deps {
                    let deg = in_degree.get_mut(dep).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next_queue.push(dep);
                    }
                }
            }
        }

        waves.push(wave);
        for item in next_queue {
            queue.push_back(item);
        }
    }

    if processed != jobs.len() {
        // Some jobs were never processed → cycle exists
        let remaining: Vec<String> = jobs
            .keys()
            .filter(|k| in_degree.get(k.as_str()).copied().unwrap_or(0) > 0)
            .cloned()
            .collect();
        return Err(DagValidationError::Cycle {
            involved_jobs: remaining,
        });
    }

    Ok(waves)
}

/// Errors from DAG validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum DagValidationError {
    #[error("job '{job}' references unknown dependency '{unknown_dep}'")]
    UnknownJob { job: String, unknown_dep: String },

    #[error("dependency cycle detected involving jobs: {}", involved_jobs.join(", "))]
    Cycle { involved_jobs: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // WorkflowConfig deserialization
    // =========================================================================

    #[test]
    fn test_deserialize_main_workflow() {
        let yaml = r#"
name: Main Pipeline

on:
  push:
    branches:
      - '**'

jobs:
  default:
    name: Lint, Test & Deploy
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
      - name: freeze
        run: airlock exec freeze
      - name: describe
        uses: airlock-hq/airlock/defaults/describe@main
      - name: test
        uses: airlock-hq/airlock/defaults/test@main
        continue-on-error: true
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
        require-approval: true
      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, Some("Main Pipeline".to_string()));
        assert!(config.on.is_some());

        let push = config.on.as_ref().unwrap().push.as_ref().unwrap();
        assert_eq!(push.branches, vec!["**"]);

        assert_eq!(config.jobs.len(), 1);
        let default_job = config.jobs.get("default").unwrap();
        assert_eq!(default_job.name, Some("Lint, Test & Deploy".to_string()));
        assert_eq!(default_job.steps.len(), 6);
        assert_eq!(default_job.steps[0].name, "lint");
        assert_eq!(
            default_job.steps[0].uses,
            Some("airlock-hq/airlock/defaults/lint@main".to_string())
        );
        assert!(default_job.steps[3].continue_on_error);
        assert!(default_job.steps[4].require_approval);
    }

    #[test]
    fn test_deserialize_hotfix_workflow() {
        let yaml = r#"
name: Hotfix Pipeline

on:
  push:
    branches:
      - 'hotfix/**'

jobs:
  default:
    name: Lint & Deploy
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, Some("Hotfix Pipeline".to_string()));

        let push = config.on.as_ref().unwrap().push.as_ref().unwrap();
        assert_eq!(push.branches, vec!["hotfix/**"]);

        let default_job = config.jobs.get("default").unwrap();
        assert_eq!(default_job.steps.len(), 2);
    }

    #[test]
    fn test_deserialize_parallel_ci_workflow() {
        let yaml = r#"
name: Parallel CI

on:
  push:
    branches: ['**']

jobs:
  lint:
    name: Lint & Format
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
      - name: freeze
        run: airlock exec freeze

  test:
    name: Test
    steps:
      - name: test
        uses: airlock-hq/airlock/defaults/test@main

  deploy:
    name: Deploy
    needs: [lint, test]
    steps:
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
        require-approval: true
      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.jobs.len(), 3);

        // Verify order is preserved
        let keys: Vec<&String> = config.jobs.keys().collect();
        assert_eq!(keys, vec!["lint", "test", "deploy"]);

        let lint = config.jobs.get("lint").unwrap();
        assert!(lint.needs.is_empty());

        let test = config.jobs.get("test").unwrap();
        assert!(test.needs.is_empty());

        let deploy = config.jobs.get("deploy").unwrap();
        assert_eq!(deploy.needs.0, vec!["lint", "test"]);
        assert_eq!(deploy.steps.len(), 2);
    }

    #[test]
    fn test_deserialize_minimal_workflow() {
        let yaml = r#"
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.name.is_none());
        assert!(config.on.is_none());
        assert_eq!(config.jobs.len(), 1);
    }

    #[test]
    fn test_deserialize_empty_workflow() {
        let yaml = "{}";
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.name.is_none());
        assert!(config.on.is_none());
        assert!(config.jobs.is_empty());
    }

    #[test]
    fn test_deserialize_branches_ignore() {
        let yaml = r#"
on:
  push:
    branches: ['**']
    branches-ignore:
      - 'experimental/**'
      - 'tmp/**'
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        let push = config.on.as_ref().unwrap().push.as_ref().unwrap();
        assert_eq!(push.branches, vec!["**"]);
        assert_eq!(push.branches_ignore, vec!["experimental/**", "tmp/**"]);
    }

    #[test]
    fn test_deserialize_keep_worktrees() {
        let yaml = r#"
jobs:
  debug:
    keep-worktrees: true
    steps:
      - name: test
        run: cargo test
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        let job = config.jobs.get("debug").unwrap();
        assert!(job.keep_worktrees);
    }

    #[test]
    fn test_needs_single_string() {
        let yaml = r#"
jobs:
  build:
    steps:
      - name: build
        run: cargo build
  test:
    needs: build
    steps:
      - name: test
        run: cargo test
"#;
        let config: WorkflowConfig = serde_yaml::from_str(yaml).unwrap();
        let test = config.jobs.get("test").unwrap();
        assert_eq!(test.needs.0, vec!["build"]);
    }

    // =========================================================================
    // Branch matching
    // =========================================================================

    #[test]
    fn test_branch_matches_no_trigger() {
        assert!(branch_matches_trigger("main", &None));
        assert!(branch_matches_trigger("feature/foo", &None));
    }

    #[test]
    fn test_branch_matches_no_push_trigger() {
        let trigger = Some(TriggerConfig { push: None });
        assert!(branch_matches_trigger("main", &trigger));
    }

    #[test]
    fn test_branch_matches_empty_branches() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec![],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("main", &trigger));
        assert!(branch_matches_trigger("feature/foo", &trigger));
    }

    #[test]
    fn test_branch_matches_exact() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["main".to_string()],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("main", &trigger));
        assert!(!branch_matches_trigger("develop", &trigger));
    }

    #[test]
    fn test_branch_matches_single_star_glob() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["feature/*".to_string()],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("feature/foo", &trigger));
        assert!(!branch_matches_trigger("feature/foo/bar", &trigger));
        assert!(!branch_matches_trigger("main", &trigger));
    }

    #[test]
    fn test_branch_matches_double_star_glob() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["feature/**".to_string()],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("feature/foo", &trigger));
        assert!(branch_matches_trigger("feature/foo/bar", &trigger));
        assert!(!branch_matches_trigger("main", &trigger));
    }

    #[test]
    fn test_branch_matches_all_glob() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["**".to_string()],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("main", &trigger));
        assert!(branch_matches_trigger("feature/foo", &trigger));
        assert!(branch_matches_trigger("a/b/c/d", &trigger));
    }

    #[test]
    fn test_branch_matches_prefix_wildcard() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["release/v*".to_string()],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("release/v1.0", &trigger));
        assert!(branch_matches_trigger("release/v2", &trigger));
        assert!(!branch_matches_trigger("release/beta", &trigger));
    }

    #[test]
    fn test_branch_matches_branches_ignore() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["**".to_string()],
                branches_ignore: vec!["experimental/**".to_string()],
            }),
        });
        assert!(branch_matches_trigger("main", &trigger));
        assert!(branch_matches_trigger("feature/foo", &trigger));
        assert!(!branch_matches_trigger("experimental/test", &trigger));
        assert!(!branch_matches_trigger("experimental/a/b", &trigger));
    }

    #[test]
    fn test_branch_matches_multiple_patterns() {
        let trigger = Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["main".to_string(), "release/**".to_string()],
                branches_ignore: vec![],
            }),
        });
        assert!(branch_matches_trigger("main", &trigger));
        assert!(branch_matches_trigger("release/v1", &trigger));
        assert!(!branch_matches_trigger("feature/foo", &trigger));
    }

    // =========================================================================
    // DAG validation
    // =========================================================================

    #[test]
    fn test_dag_single_job() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "default".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec![]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let waves = validate_job_dag(&jobs).unwrap();
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0], vec!["default"]);
    }

    #[test]
    fn test_dag_parallel_jobs() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "lint".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec![]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "test".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec![]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let waves = validate_job_dag(&jobs).unwrap();
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].len(), 2);
        assert!(waves[0].contains(&"lint".to_string()));
        assert!(waves[0].contains(&"test".to_string()));
    }

    #[test]
    fn test_dag_sequential_jobs() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "build".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec![]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "test".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["build".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "deploy".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["test".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let waves = validate_job_dag(&jobs).unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec!["build"]);
        assert_eq!(waves[1], vec!["test"]);
        assert_eq!(waves[2], vec!["deploy"]);
    }

    #[test]
    fn test_dag_diamond() {
        // build → lint, test → deploy
        let mut jobs = IndexMap::new();
        jobs.insert(
            "lint".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec![]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "test".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec![]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "deploy".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["lint".to_string(), "test".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let waves = validate_job_dag(&jobs).unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0].len(), 2);
        assert!(waves[0].contains(&"lint".to_string()));
        assert!(waves[0].contains(&"test".to_string()));
        assert_eq!(waves[1], vec!["deploy"]);
    }

    #[test]
    fn test_dag_cycle_detected() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "a".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["b".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "b".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["a".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let result = validate_job_dag(&jobs);
        assert!(result.is_err());
        match result.unwrap_err() {
            DagValidationError::Cycle { involved_jobs } => {
                assert!(involved_jobs.contains(&"a".to_string()));
                assert!(involved_jobs.contains(&"b".to_string()));
            }
            other => panic!("expected Cycle error, got: {}", other),
        }
    }

    #[test]
    fn test_dag_unknown_dependency() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "deploy".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["nonexistent".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let result = validate_job_dag(&jobs);
        assert!(result.is_err());
        match result.unwrap_err() {
            DagValidationError::UnknownJob { job, unknown_dep } => {
                assert_eq!(job, "deploy");
                assert_eq!(unknown_dep, "nonexistent");
            }
            other => panic!("expected UnknownJob error, got: {}", other),
        }
    }

    #[test]
    fn test_dag_empty() {
        let jobs: IndexMap<String, JobConfig> = IndexMap::new();
        let waves = validate_job_dag(&jobs).unwrap();
        assert!(waves.is_empty());
    }

    #[test]
    fn test_dag_self_reference() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "a".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["a".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let result = validate_job_dag(&jobs);
        assert!(result.is_err());
    }

    #[test]
    fn test_dag_three_node_cycle() {
        let mut jobs = IndexMap::new();
        jobs.insert(
            "a".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["c".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "b".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["a".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );
        jobs.insert(
            "c".to_string(),
            JobConfig {
                name: None,
                needs: OneOrMany(vec!["b".to_string()]),
                steps: vec![],
                keep_worktrees: false,
            },
        );

        let result = validate_job_dag(&jobs);
        assert!(result.is_err());
        match result.unwrap_err() {
            DagValidationError::Cycle { involved_jobs } => {
                assert_eq!(involved_jobs.len(), 3);
            }
            other => panic!("expected Cycle error, got: {}", other),
        }
    }
}
