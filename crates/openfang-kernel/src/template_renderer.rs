//! Shared MiniJinja-backed template utilities for workflow compilation and runtime rendering.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use minijinja::{Environment, Error, Template};
use openfang_types::error::{OpenFangError, OpenFangResult};
use openfang_types::workflow::{
    CompiledTemplate, TemplateNamespace, TemplateReference, TemplateSegment,
};
use serde_json::Value as JsonValue;

/// Shared MiniJinja renderer configured for workflow templates.
#[derive(Debug)]
pub(crate) struct TemplateRenderer {
    env: Environment<'static>,
}

impl TemplateRenderer {
    /// Returns the process-wide renderer instance.
    pub(crate) fn shared() -> &'static Self {
        static RENDERER: OnceLock<TemplateRenderer> = OnceLock::new();
        RENDERER.get_or_init(Self::new)
    }

    fn new() -> Self {
        Self {
            env: Environment::new(),
        }
    }

    /// Parses a template source string with MiniJinja.
    pub(crate) fn parse<'source>(
        &self,
        source: &'source str,
    ) -> Result<Template<'_, 'source>, Error> {
        self.env.template_from_str(source)
    }

    /// Returns undeclared template variables using MiniJinja's parser.
    pub(crate) fn undeclared_variables(&self, source: &str) -> Result<HashSet<String>, Error> {
        let template = self.parse(source)?;
        Ok(template.undeclared_variables(true))
    }

    /// Renders a compiled template with the workflow `input` and `vars` namespaces.
    pub(crate) fn render(
        &self,
        template: &CompiledTemplate,
        input: &str,
        vars: &HashMap<String, String>,
    ) -> OpenFangResult<String> {
        let source = render_source(template);
        let template = self
            .parse(source.as_ref())
            .map_err(|error| OpenFangError::Internal(format!("template parse: {error}")))?;
        let context = minijinja::context! {
            input => template_json_value(input),
            vars => template_json_vars(vars),
        };

        template
            .render(context)
            .map_err(|error| OpenFangError::Internal(format!("template render: {error}")))
    }
}

fn render_source(template: &CompiledTemplate) -> Cow<'_, str> {
    if template.segments.is_empty() {
        Cow::Borrowed(&template.source)
    } else {
        Cow::Owned(render_source_from_segments(&template.segments))
    }
}

fn render_source_from_segments(segments: &[TemplateSegment]) -> String {
    let mut source = String::new();

    for segment in segments {
        match segment {
            TemplateSegment::Text { value } => source.push_str(value),
            TemplateSegment::Reference { reference } => {
                source.push_str("{{ ");
                source.push_str(&display_reference(reference));
                source.push_str(" }}");
            }
        }
    }

    source
}

fn template_json_vars(vars: &HashMap<String, String>) -> BTreeMap<String, JsonValue> {
    vars.iter()
        .map(|(key, value)| (key.clone(), template_json_value(value)))
        .collect()
}

fn template_json_value(raw: &str) -> JsonValue {
    serde_json::from_str(raw).unwrap_or_else(|_| JsonValue::String(raw.to_string()))
}

fn display_reference(reference: &TemplateReference) -> String {
    let namespace = match reference.namespace {
        TemplateNamespace::Input => "input",
        TemplateNamespace::Vars => "vars",
    };

    if reference.path.is_empty() {
        namespace.to_string()
    } else {
        format!("{namespace}.{}", reference.path.join("."))
    }
}
