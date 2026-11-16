use biome_deserialize::{
    Deserializable, DeserializableType, DeserializableTypes, DeserializableValue,
    DeserializationContext, DeserializationDiagnostic, DeserializationVisitor, MapMembers, Text,
    TextNumber, yaml::deserialize_from_yaml_ast,
};
use biome_deserialize_macros::Deserializable;
use biome_diagnostics::Error;
use biome_rowan::{AstNode, SyntaxKind, TextRange};
use biome_yaml_syntax::YamlRoot;
use camino::Utf8Path;
use indexmap::IndexMap;

/// Returns `true` if `path` is a YAML file directly inside a `.github/workflows` directory.
pub fn is_github_workflow_path(path: &Utf8Path) -> bool {
    if !matches!(path.extension(), Some("yml" | "yaml")) {
        return false;
    }
    let mut ancestors = path.ancestors().skip(1).filter_map(Utf8Path::file_name);
    ancestors.next() == Some("workflows") && ancestors.next() == Some(".github")
}

#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct GitHubWorkflow {
    /// The name shown in the Actions tab. A workflow that sets `name` to a number or a boolean
    /// is still named, so this is a [Scalar].
    pub name: Option<Scalar>,
    #[deserializable(rename = "run-name")]
    pub run_name: Option<String>,
    pub on: Option<Triggers>,
    pub permissions: Option<Permissions>,
    pub env: Option<Env>,
    pub defaults: Option<Defaults>,
    pub concurrency: Option<StringOr<Concurrency>>,
    /// The jobs of the workflow, in source order and keyed by job ID.
    pub jobs: Option<IndexMap<String, Job>>,
}

impl GitHubWorkflow {
    /// Deserializes the first YAML document of `root`.
    ///
    /// The workflow is `None` if the tree contains bogus nodes or if the document can't be
    /// deserialized, for example when it isn't a mapping. The diagnostics only include
    /// deserialization errors, because the parse errors are not part of the tree.
    pub fn from_ast(path: &Utf8Path, root: &YamlRoot) -> (Option<Self>, Vec<Error>) {
        if root
            .syntax()
            .descendants()
            .any(|node| node.kind().is_bogus())
        {
            return (None, Vec::new());
        }

        deserialize_from_yaml_ast(root, path.file_name().unwrap_or("workflow.yml")).consume()
    }
}

/// A job of a workflow. It either runs `steps`, or calls a reusable workflow with `uses`.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct Job {
    pub name: Option<String>,
    pub permissions: Option<Permissions>,
    pub needs: Option<OneOrMany<String>>,
    #[deserializable(rename = "if")]
    pub condition: Option<Scalar>,
    #[deserializable(rename = "runs-on")]
    pub runs_on: Option<RunsOn>,
    pub environment: Option<StringOr<Environment>>,
    pub concurrency: Option<StringOr<Concurrency>>,
    pub outputs: Option<IndexMap<String, Scalar>>,
    pub env: Option<Env>,
    pub defaults: Option<Defaults>,
    #[deserializable(rename = "timeout-minutes")]
    pub timeout_minutes: Option<Scalar>,
    #[deserializable(rename = "continue-on-error")]
    pub continue_on_error: Option<Scalar>,
    pub steps: Option<Vec<Step>>,
    pub uses: Option<String>,
    pub with: Option<IndexMap<String, Scalar>>,
    pub secrets: Option<StringOr<IndexMap<String, Scalar>>>,
}

/// A step of a job. It either runs a command with `run`, or an action with `uses`.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct Step {
    pub id: Option<String>,
    #[deserializable(rename = "if")]
    pub condition: Option<Scalar>,
    pub name: Option<String>,
    pub uses: Option<String>,
    pub run: Option<String>,
    #[deserializable(rename = "working-directory")]
    pub working_directory: Option<String>,
    pub shell: Option<String>,
    /// The inputs of the action.
    pub with: Option<IndexMap<String, Scalar>>,
    pub env: Option<Env>,
    #[deserializable(rename = "continue-on-error")]
    pub continue_on_error: Option<Scalar>,
    #[deserializable(rename = "timeout-minutes")]
    pub timeout_minutes: Option<Scalar>,
}

/// The `env` mapping of a workflow, a job, or a step.
pub type Env = IndexMap<String, Scalar>;

/// The `defaults` of a workflow or a job.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct Defaults {
    pub run: Option<RunDefaults>,
}

/// The default settings of the `run` steps.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct RunDefaults {
    pub shell: Option<String>,
    #[deserializable(rename = "working-directory")]
    pub working_directory: Option<String>,
}

/// The mapping form of `concurrency`.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct Concurrency {
    pub group: Option<String>,
    #[deserializable(rename = "cancel-in-progress")]
    pub cancel_in_progress: Option<Scalar>,
}

/// The mapping form of `environment`.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct Environment {
    pub name: Option<String>,
    pub url: Option<String>,
}

/// The access level of a single permission.
#[derive(Clone, Copy, Debug, Deserializable, Eq, PartialEq)]
pub enum PermissionLevel {
    Read,
    Write,
    #[deserializable(rename = "none")]
    NoAccess,
}

/// The `permissions` of a workflow or a job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Permissions {
    /// The `read-all` shorthand.
    ReadAll,
    /// The `write-all` shorthand.
    WriteAll,
    /// The level of each listed permission. An empty mapping disables all the permissions.
    Scopes(IndexMap<String, PermissionLevel>),
}

impl Deserializable for Permissions {
    fn deserialize<V: DeserializableValue>(
        ctx: &mut dyn DeserializationContext<State = V::State>,
        value: &V,
        name: &str,
    ) -> Option<Self> {
        if value.visitable_type(ctx) == Some(DeserializableType::Str) {
            let text = Text::deserialize(ctx, value, name)?;
            match text.text() {
                "read-all" => Some(Self::ReadAll),
                "write-all" => Some(Self::WriteAll),
                other => {
                    ctx.report(DeserializationDiagnostic::new_unknown_value(
                        other,
                        value.range(),
                        &["read-all", "write-all"],
                    ));
                    None
                }
            }
        } else {
            Deserializable::deserialize(ctx, value, name).map(Self::Scopes)
        }
    }
}

/// The `runs-on` of a job.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunsOn {
    /// The runner group, from the mapping form.
    pub group: Option<String>,
    /// The runner labels.
    pub labels: Vec<String>,
}

#[derive(Default, Deserializable)]
#[deserializable(unknown_fields = "allow")]
struct RunsOnMapping {
    group: Option<String>,
    labels: Option<OneOrMany<String>>,
}

impl Deserializable for RunsOn {
    fn deserialize<V: DeserializableValue>(
        ctx: &mut dyn DeserializationContext<State = V::State>,
        value: &V,
        name: &str,
    ) -> Option<Self> {
        if value.visitable_type(ctx) == Some(DeserializableType::Map) {
            let mapping = RunsOnMapping::deserialize(ctx, value, name)?;
            Some(Self {
                group: mapping.group,
                labels: mapping.labels.map_or_else(Vec::new, |labels| labels.0),
            })
        } else {
            let labels = OneOrMany::deserialize(ctx, value, name)?;
            Some(Self {
                group: None,
                labels: labels.0,
            })
        }
    }
}

/// The events of the `on` field, in source order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Triggers {
    /// The configuration of each event, keyed by event name. The `on: push` and
    /// `on: [push, pull_request]` shorthands produce events with a default configuration.
    pub events: IndexMap<String, TriggerConfig>,
}

impl Triggers {
    pub fn contains(&self, event: &str) -> bool {
        self.events.contains_key(event)
    }
}

/// The filters of an event.
#[derive(Clone, Debug, Default, Deserializable, Eq, PartialEq)]
#[deserializable(unknown_fields = "allow")]
pub struct TriggerConfig {
    pub branches: Option<Vec<String>>,
    #[deserializable(rename = "branches-ignore")]
    pub branches_ignore: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    #[deserializable(rename = "tags-ignore")]
    pub tags_ignore: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    #[deserializable(rename = "paths-ignore")]
    pub paths_ignore: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    /// The workflows that trigger a `workflow_run` event.
    pub workflows: Option<Vec<String>>,
    /// The cron expressions of the `schedule` event.
    #[deserializable(skip)]
    pub crons: Vec<String>,
}

#[derive(Default, Deserializable)]
#[deserializable(unknown_fields = "allow")]
struct Schedule {
    cron: Option<String>,
}

impl Deserializable for Triggers {
    fn deserialize<V: DeserializableValue>(
        ctx: &mut dyn DeserializationContext<State = V::State>,
        value: &V,
        name: &str,
    ) -> Option<Self> {
        value.deserialize(ctx, TriggersVisitor, name)
    }
}

struct TriggersVisitor;

impl<S> DeserializationVisitor<S> for TriggersVisitor {
    type Output = Triggers;
    const EXPECTED_TYPE: DeserializableTypes = DeserializableTypes::STR
        .union(DeserializableTypes::ARRAY)
        .union(DeserializableTypes::MAP);

    fn visit_str(
        self,
        _ctx: &mut dyn DeserializationContext<State = S>,
        value: Text,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        let mut events = IndexMap::new();
        events.insert(value.text().to_string(), TriggerConfig::default());
        Some(Triggers { events })
    }

    fn visit_array(
        self,
        ctx: &mut dyn DeserializationContext<State = S>,
        items: &mut dyn ExactSizeIterator<Item = Option<Box<dyn DeserializableValue<State = S>>>>,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        let mut events = IndexMap::with_capacity(items.len());
        for (index, item) in items.enumerate() {
            let Some(item) = item else {
                continue;
            };
            ctx.enter_index(index, item.range());
            let event = String::deserialize(ctx, &item, "");
            ctx.exit_index();
            if let Some(event) = event {
                events.insert(event, TriggerConfig::default());
            }
        }
        Some(Triggers { events })
    }

    fn visit_map(
        self,
        ctx: &mut dyn DeserializationContext<State = S>,
        members: &mut MapMembers<'_, S>,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        let mut events = IndexMap::with_capacity(members.len());
        for (key, value) in members.flatten() {
            let Some(event) = Text::deserialize(ctx, &key, "") else {
                continue;
            };
            let config = if value.visitable_type(ctx) == Some(DeserializableType::Null) {
                Some(TriggerConfig::default())
            } else if event.text() == "schedule" {
                Vec::<Schedule>::deserialize(ctx, &value, event.text()).map(|schedules| {
                    TriggerConfig {
                        crons: schedules
                            .into_iter()
                            .filter_map(|schedule| schedule.cron)
                            .collect(),
                        ..TriggerConfig::default()
                    }
                })
            } else {
                TriggerConfig::deserialize(ctx, &value, event.text())
            };
            if let Some(config) = config {
                events.insert(event.text().to_string(), config);
            }
        }
        Some(Triggers { events })
    }
}

/// A value that is either a string, or the mapping form `T`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StringOr<T> {
    String(String),
    Value(T),
}

impl<T: Deserializable> Deserializable for StringOr<T> {
    fn deserialize<V: DeserializableValue>(
        ctx: &mut dyn DeserializationContext<State = V::State>,
        value: &V,
        name: &str,
    ) -> Option<Self> {
        if value.visitable_type(ctx) == Some(DeserializableType::Str) {
            String::deserialize(ctx, value, name).map(Self::String)
        } else {
            T::deserialize(ctx, value, name).map(Self::Value)
        }
    }
}

/// A value that is either a single `T`, or a list of `T`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<T: Deserializable> Deserializable for OneOrMany<T> {
    fn deserialize<V: DeserializableValue>(
        ctx: &mut dyn DeserializationContext<State = V::State>,
        value: &V,
        name: &str,
    ) -> Option<Self> {
        if value.visitable_type(ctx) == Some(DeserializableType::Array) {
            Vec::<T>::deserialize(ctx, value, name).map(Self)
        } else {
            T::deserialize(ctx, value, name).map(|item| Self(vec![item]))
        }
    }
}

/// A scalar that is kept as its source text. Numbers keep their original spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Scalar {
    Null,
    Bool(bool),
    Number(String),
    String(String),
}

impl Scalar {
    /// Returns the string, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }
}

impl Deserializable for Scalar {
    fn deserialize<V: DeserializableValue>(
        ctx: &mut dyn DeserializationContext<State = V::State>,
        value: &V,
        name: &str,
    ) -> Option<Self> {
        value.deserialize(ctx, ScalarVisitor, name)
    }
}

struct ScalarVisitor;

impl<S> DeserializationVisitor<S> for ScalarVisitor {
    type Output = Scalar;
    const EXPECTED_TYPE: DeserializableTypes = DeserializableTypes::NULL
        .union(DeserializableTypes::BOOL)
        .union(DeserializableTypes::NUMBER)
        .union(DeserializableTypes::STR);

    fn visit_null(
        self,
        _ctx: &mut dyn DeserializationContext<State = S>,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        Some(Scalar::Null)
    }

    fn visit_bool(
        self,
        _ctx: &mut dyn DeserializationContext<State = S>,
        value: bool,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        Some(Scalar::Bool(value))
    }

    fn visit_number(
        self,
        _ctx: &mut dyn DeserializationContext<State = S>,
        value: TextNumber,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        Some(Scalar::Number(value.text().to_string()))
    }

    fn visit_str(
        self,
        _ctx: &mut dyn DeserializationContext<State = S>,
        value: Text,
        _range: TextRange,
        _name: &str,
    ) -> Option<Self::Output> {
        Some(Scalar::String(value.text().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use biome_yaml_parser::parse_yaml;

    fn parse_with_diagnostics(source: &str) -> (Option<GitHubWorkflow>, Vec<Error>) {
        GitHubWorkflow::from_ast(Utf8Path::new("workflow.yml"), &parse_yaml(source).tree())
    }

    fn parse(source: &str) -> GitHubWorkflow {
        let (workflow, diagnostics) = parse_with_diagnostics(source);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        workflow.unwrap()
    }

    #[test]
    fn reports_invalid_workflow_documents() {
        let (workflow, diagnostics) = parse_with_diagnostics("not-a-mapping");
        assert_eq!(workflow, None);
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic
                .category()
                .is_some_and(|c| c.name() == "deserialize")),
            "{diagnostics:#?}"
        );
    }

    #[test]
    fn skips_trees_with_bogus_nodes() {
        let (workflow, diagnostics) = parse_with_diagnostics("name: @a");
        assert_eq!(workflow, None);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn deserializes_a_workflow() {
        let workflow = parse(
            r#"
name: CI
run-name: Run ${{ github.actor }}
on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:
  schedule:
    - cron: "0 0 * * *"
permissions: read-all
env:
  CI: true
  RETRIES: 3
concurrency: ci
jobs:
  build:
    name: Build
    runs-on: ubuntu-latest
    needs: lint
    if: github.ref == 'refs/heads/main'
    timeout-minutes: 10
    permissions:
      contents: read
      id-token: none
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - id: test
        name: Test
        run: cargo test
        shell: bash
        env:
          FOO: bar
  call:
    uses: ./.github/workflows/reusable.yml
    secrets: inherit
    environment:
      name: production
      url: https://example.com
"#,
        );

        assert_eq!(workflow.name, Some(Scalar::String("CI".to_string())));
        assert_eq!(workflow.permissions, Some(Permissions::ReadAll));
        assert_eq!(
            workflow.concurrency,
            Some(StringOr::String("ci".to_string()))
        );
        let env = workflow.env.unwrap();
        assert_eq!(env["CI"], Scalar::Bool(true));
        assert_eq!(env["RETRIES"], Scalar::Number("3".to_string()));

        let triggers = workflow.on.unwrap();
        assert_eq!(
            triggers.events.keys().collect::<Vec<_>>(),
            ["push", "pull_request", "workflow_dispatch", "schedule"]
        );
        assert_eq!(
            triggers.events["push"].branches,
            Some(vec!["main".to_string()])
        );
        assert_eq!(triggers.events["schedule"].crons, ["0 0 * * *"]);

        let jobs = workflow.jobs.unwrap();
        assert_eq!(jobs.keys().collect::<Vec<_>>(), ["build", "call"]);

        let build = &jobs["build"];
        assert_eq!(build.needs, Some(OneOrMany(vec!["lint".to_string()])));
        assert_eq!(
            build.runs_on,
            Some(RunsOn {
                group: None,
                labels: vec!["ubuntu-latest".to_string()]
            })
        );
        assert_eq!(
            build.timeout_minutes,
            Some(Scalar::Number("10".to_string()))
        );
        let Some(Permissions::Scopes(scopes)) = &build.permissions else {
            panic!("Expected permission scopes");
        };
        assert_eq!(scopes["contents"], PermissionLevel::Read);
        assert_eq!(scopes["id-token"], PermissionLevel::NoAccess);

        let steps = build.steps.as_ref().unwrap();
        assert_eq!(steps[0].uses.as_deref(), Some("actions/checkout@v4"));
        assert_eq!(
            steps[0].with.as_ref().unwrap()["fetch-depth"],
            Scalar::Number("0".to_string())
        );
        assert_eq!(steps[1].run.as_deref(), Some("cargo test"));
        assert_eq!(steps[1].shell.as_deref(), Some("bash"));

        let call = &jobs["call"];
        assert_eq!(call.secrets, Some(StringOr::String("inherit".to_string())));
        assert_eq!(
            call.environment,
            Some(StringOr::Value(Environment {
                name: Some("production".to_string()),
                url: Some("https://example.com".to_string()),
            }))
        );
    }

    #[test]
    fn deserializes_the_trigger_shorthands() {
        let triggers = parse("on: push").on.unwrap();
        assert!(triggers.contains("push"));

        let triggers = parse("on: [push, pull_request]").on.unwrap();
        assert!(triggers.contains("push"));
        assert!(triggers.contains("pull_request"));
        assert!(!triggers.contains("schedule"));
    }

    #[test]
    fn deserializes_runs_on_forms() {
        let workflow = parse(
            r#"
jobs:
  list:
    runs-on: [self-hosted, linux]
  group:
    runs-on:
      group: ubuntu-runners
      labels: ubuntu-20.04-16core
"#,
        );
        let jobs = workflow.jobs.unwrap();
        assert_eq!(
            jobs["list"].runs_on.as_ref().unwrap().labels,
            ["self-hosted", "linux"]
        );
        let group = jobs["group"].runs_on.as_ref().unwrap();
        assert_eq!(group.group.as_deref(), Some("ubuntu-runners"));
        assert_eq!(group.labels, ["ubuntu-20.04-16core"]);
    }

    #[test]
    fn resolves_anchors_and_ignores_unknown_keys() {
        let workflow = parse(
            r#"
unknown: value
jobs:
  build:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [a, b]
    steps:
      - &checkout
        uses: actions/checkout@v4
      - *checkout
"#,
        );
        let steps = workflow.jobs.unwrap()["build"].steps.clone().unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0], steps[1]);
    }

    #[test]
    fn reports_unresolved_aliases_in_shorthand_fields() {
        let (workflow, diagnostics) = parse_with_diagnostics(
            r#"
name: CI
permissions: *missing
concurrency: *missing
jobs:
  build:
    needs: *missing
    runs-on: *missing
"#,
        );
        let workflow = workflow.unwrap();
        assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
        assert_eq!(workflow.name, Some(Scalar::String("CI".to_string())));
    }

    #[test]
    fn reports_invalid_values_and_keeps_the_rest() {
        let (workflow, diagnostics) = parse_with_diagnostics(
            r#"
name: CI
permissions: everything
jobs:
  build:
    runs-on: ubuntu-latest
"#,
        );
        let workflow = workflow.unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(workflow.permissions, None);
        assert_eq!(workflow.name, Some(Scalar::String("CI".to_string())));
        assert!(workflow.jobs.unwrap().contains_key("build"));
    }

    #[test]
    fn detects_workflow_paths() {
        for path in [
            ".github/workflows/ci.yml",
            "repo/.github/workflows/ci.yaml",
            "/abs/.github/workflows/release.yml",
        ] {
            assert!(is_github_workflow_path(Utf8Path::new(path)), "{path}");
        }
        for path in [
            "ci.yml",
            ".github/ci.yml",
            ".github/workflows/ci.json",
            ".github/workflows/nested/ci.yml",
            "github/workflows/ci.yml",
            ".github/actions/setup/action.yml",
        ] {
            assert!(!is_github_workflow_path(Utf8Path::new(path)), "{path}");
        }
    }
}
