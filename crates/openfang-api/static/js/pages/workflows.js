// OpenFang Workflows Page — workflow v2 authoring on the v1 API
'use strict';

var WORKFLOW_UI_STEP_KIND_OPTIONS = [
  { value: 'agent', label: 'Agent' },
  { value: 'primitive', label: 'Primitive' },
  { value: 'workflow', label: 'Workflow' },
  { value: 'wait_signal', label: 'Wait Signal' },
  { value: 'start_looper', label: 'Start Looper' },
  { value: 'emit_event', label: 'Emit Event' },
  { value: 'collect', label: 'Collect' },
  { value: 'noop', label: 'Noop' }
];

var WORKFLOW_UI_FLOW_OPTIONS = [
  { value: 'sequential', label: 'Sequential' },
  { value: 'fan_out', label: 'Fan Out' },
  { value: 'conditional', label: 'Conditional' },
  { value: 'loop', label: 'Loop' }
];

var WORKFLOW_UI_CONTRACT_KIND_OPTIONS = [
  { value: 'string', label: 'string' },
  { value: 'integer', label: 'integer' },
  { value: 'number', label: 'number' },
  { value: 'boolean', label: 'boolean' },
  { value: 'object', label: 'object' },
  { value: 'array', label: 'array' },
  { value: 'any', label: 'any' },
  { value: 'artifact_ref', label: 'artifact_ref' },
  { value: 'doc_ref', label: 'doc_ref' },
  { value: 'issue_ref', label: 'issue_ref' },
  { value: 'task_ref', label: 'task_ref' },
  { value: 'task_list', label: 'task_list' },
  { value: 'run_ref', label: 'run_ref' }
];

function workflowUiNormalizeText(value) {
  return value === undefined || value === null ? '' : String(value);
}

function workflowUiNormalizeList(payload) {
  if (Array.isArray(payload)) return payload;
  if (payload && Array.isArray(payload.items)) return payload.items;
  return [];
}

function workflowUiSlugify(value, fallback) {
  var slug = workflowUiNormalizeText(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

  if (slug) return slug;
  return fallback || 'workflow';
}

function workflowUiTitleCase(value) {
  return workflowUiNormalizeText(value)
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map(function(part) {
      return part.charAt(0).toUpperCase() + part.slice(1);
    })
    .join(' ');
}

function workflowUiFormatJson(value) {
  return JSON.stringify(value, null, 2);
}

function workflowUiParseJson(text, fallbackValue) {
  if (!workflowUiNormalizeText(text).trim()) {
    return fallbackValue;
  }
  return JSON.parse(text);
}

function workflowUiDefaultContract(kind) {
  if (kind === 'array') {
    return {
      kind: 'array',
      items: { kind: 'string' }
    };
  }

  if (kind === 'object') {
    return {
      kind: 'object',
      required: [],
      open: false,
      fields: {}
    };
  }

  if (kind === 'artifact_ref') {
    return {
      kind: 'artifact_ref',
      artifact_type: 'artifact'
    };
  }

  if (kind === 'doc_ref') {
    return {
      kind: 'doc_ref',
      doc_type: 'document'
    };
  }

  return {
    kind: kind || 'string'
  };
}

function workflowUiNewBindingRow(key, value) {
  return {
    key: workflowUiNormalizeText(key),
    value: workflowUiNormalizeText(value)
  };
}

function workflowUiNewOutputRow(key, value) {
  return {
    key: workflowUiNormalizeText(key),
    value: workflowUiNormalizeText(value)
  };
}

function workflowUiReservedBindingKeys(kind) {
  if (kind === 'agent') return ['instructions', 'provider_override'];
  if (kind === 'emit_event') return ['source'];
  if (kind === 'start_looper') return ['execution_policy'];
  if (kind === 'collect') return ['from_step', 'aggregation'];
  return [];
}

function workflowUiDefaultStep(kind, index) {
  return {
    id: workflowUiSlugify(kind || 'step', 'step') + '-' + (index + 1),
    name: workflowUiTitleCase(kind || 'step') + ' Step',
    kind: kind || 'noop',
    save_as: '',
    flow_mode: 'sequential',
    flow_when: '',
    flow_until: '',
    flow_max_iterations: 3,
    runtime_timeout_secs: '',
    agent_id: '',
    instructions: '',
    provider_override: '',
    action: '',
    params_text: '{\n}',
    workflow_id: '',
    signal_name: '',
    timeout_secs: '',
    task_id: '',
    execution_policy: '{\n}',
    event: '',
    source: '',
    payload: '',
    from_step: '',
    aggregation: '',
    with_rows: []
  };
}

function workflowUiDefaultDefinitionForm() {
  return {
    id: '',
    name: '',
    version: '1.0.0',
    description: '',
    enabled: true,
    tags_text: '',
    input_text: workflowUiFormatJson({
      kind: 'object',
      required: ['topic'],
      open: false,
      fields: {
        topic: {
          kind: 'string',
          description: 'Primary workflow topic'
        }
      }
    }),
    output_text: workflowUiFormatJson({
      kind: 'object',
      required: ['result'],
      open: false,
      fields: {
        result: {
          kind: 'string',
          description: 'Final workflow result'
        }
      }
    }),
    defaults_timeout_secs: 120,
    steps: [
      (function() {
        var step = workflowUiDefaultStep('noop', 0);
        step.id = 'noop-step';
        step.name = 'Noop Step';
        step.save_as = 'result';
        return step;
      })()
    ],
    outputs: [
      workflowUiNewOutputRow('result', '{{ vars.result }}')
    ]
  };
}

function workflowUiStepUsesKind(step) {
  if (!step || !step.uses) return null;
  if (step.kind === 'agent') return step.uses.agent || '';
  if (step.kind === 'primitive') return step.uses.primitive || '';
  if (step.kind === 'workflow') return step.uses.workflow || '';
  if (step.kind === 'wait_signal') return step.uses.signal_name || '';
  if (step.kind === 'start_looper') {
    return step.uses.task_ref || step.uses.task_id_binding || '';
  }
  if (step.kind === 'emit_event') return step.uses.event || '';
  return '';
}

function workflowUiExtractBindings(step, reservedKeys) {
  var result = [];
  var bindings = step && step.with ? step.with : {};
  var keys = Object.keys(bindings);
  var reserved = {};
  var i;

  for (i = 0; i < reservedKeys.length; i++) {
    reserved[reservedKeys[i]] = true;
  }

  for (i = 0; i < keys.length; i++) {
    if (!reserved[keys[i]]) {
      result.push(workflowUiNewBindingRow(keys[i], bindings[keys[i]]));
    }
  }

  return result;
}

function workflowUiDeserializeStep(step, index) {
  var next = workflowUiDefaultStep(step && step.kind ? step.kind : 'noop', index || 0);
  var flow = step && step.flow ? step.flow : { mode: 'sequential' };
  var runtime = step && step.runtime ? step.runtime : {};
  var bindings = step && step.with ? step.with : {};

  next.id = workflowUiNormalizeText(step && step.id) || next.id;
  next.name = workflowUiNormalizeText(step && step.name) || next.name;
  next.kind = workflowUiNormalizeText(step && step.kind) || next.kind;
  next.save_as = workflowUiNormalizeText(step && step.save_as);
  next.flow_mode = workflowUiNormalizeText(flow.mode) || 'sequential';
  next.flow_when = workflowUiNormalizeText(flow.when);
  next.flow_until = workflowUiNormalizeText(flow.until);
  next.flow_max_iterations = flow.max_iterations || 3;
  next.runtime_timeout_secs = runtime && runtime.timeout_secs ? String(runtime.timeout_secs) : '';

  if (next.kind === 'agent') {
    next.agent_id = workflowUiNormalizeText(step && step.uses && step.uses.agent);
    next.instructions = workflowUiNormalizeText(bindings.instructions);
    next.provider_override = workflowUiNormalizeText(bindings.provider_override);
  } else if (next.kind === 'primitive') {
    next.action = workflowUiNormalizeText(step && step.uses && step.uses.primitive);
    next.params_text = workflowUiFormatJson(bindings || {});
  } else if (next.kind === 'workflow') {
    next.workflow_id = workflowUiNormalizeText(step && step.uses && step.uses.workflow);
  } else if (next.kind === 'wait_signal') {
    next.signal_name = workflowUiNormalizeText(step && step.uses && step.uses.signal_name);
    next.timeout_secs = next.runtime_timeout_secs;
  } else if (next.kind === 'start_looper') {
    next.task_id = workflowUiNormalizeText(
      step && step.uses && (step.uses.task_ref || step.uses.task_id_binding)
    );
    next.execution_policy = workflowUiNormalizeText(bindings.execution_policy) || '{\n}';
  } else if (next.kind === 'emit_event') {
    next.event = workflowUiNormalizeText(step && step.uses && step.uses.event);
    next.source = workflowUiNormalizeText(bindings.source);
    next.payload = workflowUiNormalizeText(step && step.uses && step.uses.payload_template);
  } else if (next.kind === 'collect') {
    next.from_step = workflowUiNormalizeText(bindings.from_step);
    next.aggregation = workflowUiNormalizeText(bindings.aggregation);
  }

  next.with_rows = workflowUiExtractBindings(step, workflowUiReservedBindingKeys(next.kind));
  if (next.kind === 'primitive') {
    next.with_rows = [];
  }

  return next;
}

function workflowUiHydrateDefinitionForm(resource) {
  var definition = resource && resource.definition ? resource.definition : resource;
  var form = workflowUiDefaultDefinitionForm();
  var i;

  form.id = workflowUiNormalizeText(definition && definition.id);
  form.name = workflowUiNormalizeText(definition && definition.name);
  form.version = workflowUiNormalizeText(definition && definition.version) || '1.0.0';
  form.description = workflowUiNormalizeText(definition && definition.description);
  form.enabled = definition && definition.enabled !== false;
  form.tags_text = Array.isArray(definition && definition.tags)
    ? definition.tags.join(', ')
    : '';
  form.input_text = workflowUiFormatJson(definition && definition.input ? definition.input : workflowUiDefaultContract('object'));
  form.output_text = workflowUiFormatJson(definition && definition.output ? definition.output : workflowUiDefaultContract('object'));
  form.defaults_timeout_secs = definition && definition.defaults && definition.defaults.timeout_secs
    ? definition.defaults.timeout_secs
    : 120;
  form.outputs = [];

  if (definition && definition.outputs) {
    var outputKeys = Object.keys(definition.outputs);
    for (i = 0; i < outputKeys.length; i++) {
      form.outputs.push(workflowUiNewOutputRow(outputKeys[i], definition.outputs[outputKeys[i]]));
    }
  }

  if (!form.outputs.length) {
    form.outputs.push(workflowUiNewOutputRow('result', '{{ vars.result }}'));
  }

  form.steps = Array.isArray(definition && definition.steps)
    ? definition.steps.map(function(step, index) {
        return workflowUiDeserializeStep(step, index);
      })
    : [workflowUiDefaultStep('noop', 0)];

  if (!form.steps.length) {
    form.steps.push(workflowUiDefaultStep('noop', 0));
  }

  return form;
}

function workflowUiBindingMapFromRows(rows) {
  var bindings = {};
  var i;

  for (i = 0; i < rows.length; i++) {
    if (!rows[i]) continue;
    if (!workflowUiNormalizeText(rows[i].key).trim()) continue;
    bindings[workflowUiNormalizeText(rows[i].key).trim()] = workflowUiNormalizeText(rows[i].value);
  }

  return bindings;
}

function workflowUiPrimitiveBindings(step) {
  var text = workflowUiNormalizeText(step.params_text).trim();
  var parsed;
  var bindings = {};
  var keys;
  var i;

  if (!text) return bindings;

  parsed = JSON.parse(text);
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
    throw new Error('Primitive params must be a JSON object');
  }

  keys = Object.keys(parsed);
  for (i = 0; i < keys.length; i++) {
    if (typeof parsed[keys[i]] === 'string') {
      bindings[keys[i]] = parsed[keys[i]];
    } else {
      bindings[keys[i]] = JSON.stringify(parsed[keys[i]]);
    }
  }

  return bindings;
}

function workflowUiBuildRuntime(step) {
  var runtime = {};
  var timeout = workflowUiNormalizeText(step.runtime_timeout_secs).trim();

  if (!timeout && workflowUiNormalizeText(step.timeout_secs).trim()) {
    timeout = workflowUiNormalizeText(step.timeout_secs).trim();
  }

  if (timeout) {
    runtime.timeout_secs = Number(timeout);
  }

  return Object.keys(runtime).length ? runtime : null;
}

function workflowUiBuildStepDefinition(step, index) {
  var next = {
    id: workflowUiSlugify(step.id || step.name || step.kind || ('step-' + (index + 1)), 'step-' + (index + 1)),
    name: workflowUiNormalizeText(step.name) || workflowUiTitleCase(step.kind) + ' Step',
    kind: workflowUiNormalizeText(step.kind) || 'noop',
    flow: { mode: workflowUiNormalizeText(step.flow_mode) || 'sequential' }
  };
  var bindings = workflowUiBindingMapFromRows(step.with_rows || []);
  var uses = null;
  var runtime = workflowUiBuildRuntime(step);

  if (next.kind === 'agent') {
    uses = { agent: workflowUiNormalizeText(step.agent_id).trim() };
    if (workflowUiNormalizeText(step.instructions).trim()) {
      bindings.instructions = workflowUiNormalizeText(step.instructions);
    }
    if (workflowUiNormalizeText(step.provider_override).trim()) {
      bindings.provider_override = workflowUiNormalizeText(step.provider_override);
    }
  } else if (next.kind === 'primitive') {
    uses = { primitive: workflowUiNormalizeText(step.action).trim() };
    bindings = workflowUiPrimitiveBindings(step);
  } else if (next.kind === 'workflow') {
    uses = { workflow: workflowUiNormalizeText(step.workflow_id).trim() };
  } else if (next.kind === 'wait_signal') {
    uses = { signal_name: workflowUiNormalizeText(step.signal_name).trim() };
  } else if (next.kind === 'start_looper') {
    var taskId = workflowUiNormalizeText(step.task_id).trim();
    uses = {};
    if (taskId.indexOf('{{') >= 0) {
      uses.task_id_binding = taskId;
    } else if (taskId) {
      uses.task_ref = taskId;
    }
    if (workflowUiNormalizeText(step.execution_policy).trim() && workflowUiNormalizeText(step.execution_policy).trim() !== '{\n}') {
      bindings.execution_policy = workflowUiNormalizeText(step.execution_policy);
    }
  } else if (next.kind === 'emit_event') {
    uses = { event: workflowUiNormalizeText(step.event).trim() };
    if (workflowUiNormalizeText(step.payload).trim()) {
      uses.payload_template = workflowUiNormalizeText(step.payload);
    }
    if (workflowUiNormalizeText(step.source).trim()) {
      bindings.source = workflowUiNormalizeText(step.source);
    }
  } else if (next.kind === 'collect') {
    if (workflowUiNormalizeText(step.from_step).trim()) {
      bindings.from_step = workflowUiNormalizeText(step.from_step);
    }
    if (workflowUiNormalizeText(step.aggregation).trim()) {
      bindings.aggregation = workflowUiNormalizeText(step.aggregation);
    }
  }

  if (next.flow.mode === 'conditional') {
    next.flow.when = workflowUiNormalizeText(step.flow_when);
  } else if (next.flow.mode === 'loop') {
    next.flow.until = workflowUiNormalizeText(step.flow_until);
    next.flow.max_iterations = Number(step.flow_max_iterations || 0);
  }

  if (Object.keys(bindings).length) {
    next.with = bindings;
  }

  if (workflowUiNormalizeText(step.save_as).trim()) {
    next.save_as = workflowUiNormalizeText(step.save_as).trim();
  }

  if (uses && Object.keys(uses).length) {
    next.uses = uses;
  }

  if (runtime) {
    next.runtime = runtime;
  }

  return next;
}

function workflowUiBuildDefinition(form) {
  var inputContract = workflowUiParseJson(form.input_text, workflowUiDefaultContract('object'));
  var outputContract = workflowUiParseJson(form.output_text, workflowUiDefaultContract('object'));
  var outputs = {};
  var outputRows = form.outputs || [];
  var definition;
  var i;

  for (i = 0; i < outputRows.length; i++) {
    if (!outputRows[i]) continue;
    if (!workflowUiNormalizeText(outputRows[i].key).trim()) continue;
    outputs[workflowUiNormalizeText(outputRows[i].key).trim()] = workflowUiNormalizeText(outputRows[i].value);
  }

  definition = {
    id: workflowUiSlugify(form.id || form.name || 'workflow', 'workflow'),
    name: workflowUiNormalizeText(form.name) || 'Untitled Workflow',
    version: workflowUiNormalizeText(form.version) || '1.0.0',
    description: workflowUiNormalizeText(form.description),
    enabled: form.enabled !== false,
    tags: workflowUiNormalizeText(form.tags_text)
      .split(',')
      .map(function(tag) { return tag.trim(); })
      .filter(Boolean),
    input: inputContract,
    output: outputContract,
    defaults: {
      timeout_secs: Number(form.defaults_timeout_secs || 120),
      error_mode: 'fail'
    },
    steps: (form.steps || []).map(function(step, index) {
      return workflowUiBuildStepDefinition(step, index);
    }),
    outputs: outputs
  };

  return definition;
}

function workflowUiIssueSeverityClass(severity) {
  return severity === 'error' ? 'badge badge-error' : 'badge badge-warn';
}

function workflowUiOriginLabel(origin) {
  if (!origin || !origin.kind) return 'unknown';
  if (origin.kind === 'pack') {
    return origin.pack_id ? 'pack:' + origin.pack_id : 'pack';
  }
  return origin.kind;
}

function workflowUiContractFields(node, path, requiredNames) {
  var fields = [];
  var keys;
  var i;
  var childPath;
  var child;

  path = path || [];
  requiredNames = requiredNames || [];

  if (!node || typeof node !== 'object') return fields;

  if (node.kind === 'object' && node.fields && typeof node.fields === 'object') {
    keys = Object.keys(node.fields);
    for (i = 0; i < keys.length; i++) {
      child = node.fields[keys[i]];
      childPath = path.concat(keys[i]);
      fields = fields.concat(
        workflowUiContractFields(
          child,
          childPath,
          Array.isArray(node.required) ? node.required : []
        )
      );
    }
    if (!keys.length && !path.length) {
      fields.push({
        key: '',
        label: 'value',
        kind: 'object',
        required: false,
        description: node.description || '',
        path: []
      });
    }
    return fields;
  }

  fields.push({
    key: path.join('.'),
    label: path.length ? path.join('.') : 'value',
    kind: node.kind || 'any',
    required: path.length ? requiredNames.indexOf(path[path.length - 1]) >= 0 : false,
    description: node.description || '',
    path: path.slice()
  });

  return fields;
}

function workflowUiSetNestedValue(target, path, value) {
  var cursor = target;
  var i;

  if (!path.length) return value;

  for (i = 0; i < path.length - 1; i++) {
    if (!cursor[path[i]] || typeof cursor[path[i]] !== 'object' || Array.isArray(cursor[path[i]])) {
      cursor[path[i]] = {};
    }
    cursor = cursor[path[i]];
  }

  cursor[path[path.length - 1]] = value;
  return target;
}

function workflowsPage() {
  return {
    workflows: [],
    agents: [],
    runtimeById: {},
    workflowRuns: [],
    selectedWorkflowId: '',
    selectedWorkflow: null,
    selectedRuntime: null,
    formRevision: 0,
    loading: true,
    loadError: '',
    detailLoading: false,
    detailError: '',
    agentsLoadError: '',
    workflowSearch: '',
    saving: false,
    validating: false,
    compiling: false,
    running: false,
    dryRunning: false,
    togglingId: '',
    deletingId: '',
    forkingId: '',
    form: workflowUiDefaultDefinitionForm(),
    validationResult: null,
    compileResult: null,
    runResult: null,
    dryRunResult: null,
    runForm: {
      values: {},
      labels_text: '',
      metadata_text: '{\n  "source": "dashboard"\n}'
    },
    workflowChangeHandler: null,

    init() {
      var self = this;
      this.workflowChangeHandler = function() {
        self.loadWorkflows({ preserveSelection: true, refreshDetail: true });
      };
      window.addEventListener('openfang:workflows-changed', this.workflowChangeHandler);
      this.loadData();
    },

    destroy() {
      if (this.workflowChangeHandler) {
        window.removeEventListener('openfang:workflows-changed', this.workflowChangeHandler);
        this.workflowChangeHandler = null;
      }
    },

    get stepKindOptions() {
      return WORKFLOW_UI_STEP_KIND_OPTIONS;
    },

    get flowOptions() {
      return WORKFLOW_UI_FLOW_OPTIONS;
    },

    get contractKindOptions() {
      return WORKFLOW_UI_CONTRACT_KIND_OPTIONS;
    },

    get filteredWorkflows() {
      var needle = workflowUiNormalizeText(this.workflowSearch).trim().toLowerCase();
      if (!needle) return this.workflows;

      return this.workflows.filter(function(item) {
        var haystack = [
          item.id,
          item.name,
          item.description,
          workflowUiOriginLabel(item.origin),
          Array.isArray(item.tags) ? item.tags.join(' ') : ''
        ]
          .join(' ')
          .toLowerCase();
        return haystack.indexOf(needle) >= 0;
      });
    },

    get validationIssues() {
      if (!this.validationResult || !Array.isArray(this.validationResult.issues)) {
        return [];
      }
      return this.validationResult.issues;
    },

    get compileSummary() {
      var workflowIr;
      if (!this.compileResult || !this.compileResult.compiled) return null;
      workflowIr = this.compileResult.compiled.workflow_ir;
      if (!workflowIr) return null;
      return {
        step_count: Array.isArray(workflowIr.steps) ? workflowIr.steps.length : 0,
        symbol_count: workflowIr.symbol_table ? Object.keys(workflowIr.symbol_table).length : 0,
        workflow_id: workflowIr.workflow_id || '',
        workflow_version: workflowIr.workflow_version || ''
      };
    },

    get currentInputContract() {
      try {
        return workflowUiParseJson(this.form.input_text, workflowUiDefaultContract('object'));
      } catch (_) {
        return null;
      }
    },

    get runInputFields() {
      if (!this.currentInputContract) return [];
      return workflowUiContractFields(this.currentInputContract, [], []);
    },

    get canDeleteSelected() {
      return !!(this.selectedWorkflow && this.selectedWorkflow.origin && this.selectedWorkflow.origin.kind !== 'pack');
    },

    get canForkSelected() {
      return !!(this.selectedWorkflow && this.selectedWorkflow.origin && this.selectedWorkflow.origin.kind === 'pack');
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';

      await Promise.all([
        this.loadAgents(),
        this.loadWorkflows({ preserveSelection: true })
      ]);

      this.loading = false;
    },

    async loadAgents() {
      this.agentsLoadError = '';

      try {
        this.agents = workflowUiNormalizeList(await OpenFangAPI.v1.agents.list());
      } catch (e) {
        this.agents = [];
        this.agentsLoadError = e.message || 'Could not load agents.';
      }
    },

    async loadWorkflows(options) {
      options = options || {};

      try {
        var response = await OpenFangAPI.v1.workflows.list();
        var items = workflowUiNormalizeList(response);
        var nextSelection = '';
        var i;

        this.workflows = items;
        this.hydrateWorkflowRuntimes(items);

        if (options.preserveSelection && this.selectedWorkflowId) {
          for (i = 0; i < items.length; i++) {
            if (items[i].id === this.selectedWorkflowId) {
              nextSelection = this.selectedWorkflowId;
              break;
            }
          }
        }

        if (!nextSelection && items.length) {
          nextSelection = items[0].id;
        }

        if (nextSelection) {
          await this.selectWorkflow(nextSelection, !!options.refreshDetail);
        } else if (!items.length) {
          this.createWorkflowDraft();
        }
      } catch (e) {
        this.workflows = [];
        this.loadError = e.message || 'Could not load workflows.';
      }
    },

    async hydrateWorkflowRuntimes(items) {
      var self = this;

      await Promise.all((items || []).map(async function(item) {
        try {
          self.runtimeById[item.id] = await OpenFangAPI.v1.workflows.runtime(item.id);
        } catch (_) {
          self.runtimeById[item.id] = {
            workflow_id: item.id,
            loaded: item.runtime_status ? item.runtime_status.loaded : false,
            healthy: false,
            active_runs: item.runtime_status ? item.runtime_status.active_runs : 0,
            waiting_runs: item.runtime_status ? item.runtime_status.waiting_runs : 0,
            last_run_at: null
          };
        }
      }));
    },

    createWorkflowDraft() {
      this.selectedWorkflowId = '';
      this.selectedWorkflow = null;
      this.selectedRuntime = null;
      this.workflowRuns = [];
      this.detailError = '';
      this.validationResult = null;
      this.compileResult = null;
      this.runResult = null;
      this.dryRunResult = null;
      this.form = workflowUiDefaultDefinitionForm();
      this.formRevision += 1;
      this.resetRunForm();
    },

    async selectWorkflow(id, forceReload) {
      if (!id) {
        this.createWorkflowDraft();
        return;
      }
      if (!forceReload && this.selectedWorkflowId === id && this.selectedWorkflow) {
        return;
      }

      this.selectedWorkflowId = id;
      this.detailLoading = true;
      this.detailError = '';
      this.validationResult = null;
      this.compileResult = null;
      this.runResult = null;
      this.dryRunResult = null;

      try {
        var responses = await Promise.all([
          OpenFangAPI.v1.workflows.get(id),
          OpenFangAPI.v1.workflows.runtime(id),
          OpenFangAPI.v1.workflows.runs(id)
        ]);
        this.selectedWorkflow = responses[0];
        this.selectedRuntime = responses[1];
        this.runtimeById[id] = responses[1];
        this.workflowRuns = workflowUiNormalizeList(responses[2]);
        this.form = workflowUiHydrateDefinitionForm(this.selectedWorkflow);
        this.formRevision += 1;
        this.resetRunForm();
      } catch (e) {
        this.detailError = e.message || 'Could not load workflow detail.';
      }

      this.detailLoading = false;
    },

    resetRunForm() {
      var values = {};
      var fields = this.runInputFields;
      var i;

      for (i = 0; i < fields.length; i++) {
        if (fields[i].kind === 'boolean') {
          values[fields[i].key] = false;
        } else {
          values[fields[i].key] = '';
        }
      }

      this.runForm = {
        values: values,
        labels_text: '',
        metadata_text: '{\n  "source": "dashboard"\n}'
      };
    },

    applyContractPreset(target, kind) {
      if (target === 'input') {
        this.form.input_text = workflowUiFormatJson(workflowUiDefaultContract(kind));
      } else {
        this.form.output_text = workflowUiFormatJson(workflowUiDefaultContract(kind));
      }
    },

    addOutputRow() {
      this.form.outputs.push(workflowUiNewOutputRow('', ''));
    },

    removeOutputRow(index) {
      this.form.outputs.splice(index, 1);
      if (!this.form.outputs.length) {
        this.form.outputs.push(workflowUiNewOutputRow('', ''));
      }
    },

    addStep(kind) {
      this.form.steps.push(workflowUiDefaultStep(kind || 'agent', this.form.steps.length));
    },

    duplicateStep(index) {
      var clone = JSON.parse(JSON.stringify(this.form.steps[index]));
      clone.id = workflowUiSlugify(clone.id || clone.name || clone.kind, 'step') + '-copy';
      clone.name = workflowUiNormalizeText(clone.name) + ' Copy';
      this.form.steps.splice(index + 1, 0, clone);
    },

    removeStep(index) {
      this.form.steps.splice(index, 1);
      if (!this.form.steps.length) {
        this.form.steps.push(workflowUiDefaultStep('noop', 0));
      }
    },

    moveStep(index, direction) {
      var nextIndex = index + direction;
      var current;
      if (nextIndex < 0 || nextIndex >= this.form.steps.length) return;
      current = this.form.steps[index];
      this.form.steps.splice(index, 1);
      this.form.steps.splice(nextIndex, 0, current);
    },

    addBindingRow(step) {
      step.with_rows.push(workflowUiNewBindingRow('', ''));
    },

    removeBindingRow(step, index) {
      step.with_rows.splice(index, 1);
    },

    changeStepKind(step, index) {
      var replacement = workflowUiDefaultStep(step.kind, index);
      replacement.id = workflowUiNormalizeText(step.id) || replacement.id;
      replacement.name = workflowUiNormalizeText(step.name) || replacement.name;
      replacement.save_as = workflowUiNormalizeText(step.save_as);
      replacement.flow_mode = workflowUiNormalizeText(step.flow_mode) || 'sequential';
      replacement.flow_when = workflowUiNormalizeText(step.flow_when);
      replacement.flow_until = workflowUiNormalizeText(step.flow_until);
      replacement.flow_max_iterations = step.flow_max_iterations || 3;
      replacement.runtime_timeout_secs = workflowUiNormalizeText(step.runtime_timeout_secs);
      this.form.steps.splice(index, 1, replacement);
    },

    issueCountForPath(prefix) {
      var count = 0;
      var issues = this.validationIssues;
      var i;

      for (i = 0; i < issues.length; i++) {
        if (issues[i].path && issues[i].path.indexOf(prefix) === 0) {
          count += 1;
        }
      }

      return count;
    },

    stepIssues(index) {
      return this.validationIssues.filter(function(issue) {
        return workflowUiNormalizeText(issue.path).indexOf('steps[' + index + ']') === 0;
      });
    },

    issuesForPath(prefix) {
      return this.validationIssues.filter(function(issue) {
        return workflowUiNormalizeText(issue.path).indexOf(prefix) === 0;
      });
    },

    issueSeverityClass(issue) {
      return workflowUiIssueSeverityClass(issue && issue.severity);
    },

    originBadgeClass(origin) {
      return origin && origin.kind === 'pack' ? 'badge badge-info' : 'badge badge-dim';
    },

    runtimeSnapshot(item) {
      return this.runtimeById[item.id] || {
        loaded: item.runtime_status ? item.runtime_status.loaded : false,
        healthy: false,
        active_runs: item.runtime_status ? item.runtime_status.active_runs : 0,
        waiting_runs: item.runtime_status ? item.runtime_status.waiting_runs : 0,
        last_run_at: null
      };
    },

    async toggleWorkflowEnabled(item) {
      var nextEnabled = !item.enabled;
      var resource;
      var definition;

      this.togglingId = item.id;

      try {
        resource = this.selectedWorkflow && this.selectedWorkflowId === item.id
          ? this.selectedWorkflow
          : await OpenFangAPI.v1.workflows.get(item.id);
        definition = JSON.parse(JSON.stringify(resource.definition));
        definition.enabled = nextEnabled;
        await OpenFangAPI.v1.workflows.update(item.id, definition);
        OpenFangToast.success(
          'Workflow ' + (nextEnabled ? 'enabled' : 'disabled') + ': ' + item.name
        );
        window.dispatchEvent(new CustomEvent('openfang:workflows-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to update workflow: ' + e.message);
      }

      this.togglingId = '';
    },

    buildDefinitionOrToast() {
      try {
        return workflowUiBuildDefinition(this.form);
      } catch (e) {
        e.workflowBuildError = true;
        OpenFangToast.error('Workflow definition is invalid: ' + e.message);
        throw e;
      }
    },

    async saveWorkflow() {
      var definition;
      var saved;

      this.saving = true;

      try {
        definition = this.buildDefinitionOrToast();
        if (this.selectedWorkflowId) {
          saved = await OpenFangAPI.v1.workflows.update(this.selectedWorkflowId, definition);
          OpenFangToast.success('Workflow updated: ' + saved.name);
        } else {
          saved = await OpenFangAPI.v1.workflows.create(definition);
          OpenFangToast.success('Workflow created: ' + saved.name);
        }
        window.dispatchEvent(new CustomEvent('openfang:workflows-changed'));
        await this.selectWorkflow(saved.id, true);
      } catch (e) {
        if (!e || !e.workflowBuildError) {
          OpenFangToast.error('Failed to save workflow: ' + e.message);
        }
      }

      this.saving = false;
    },

    confirmDeleteSelected() {
      var self = this;
      if (!this.selectedWorkflow) return;

      OpenFangUtils.confirmAction(
        'Delete Workflow',
        'Delete workflow "' + this.selectedWorkflow.definition.name + '"? This cannot be undone.',
        function() {
          self.deleteSelectedWorkflow();
        }
      );
    },

    async deleteSelectedWorkflow() {
      if (!this.selectedWorkflowId) return;
      this.deletingId = this.selectedWorkflowId;

      try {
        await OpenFangAPI.v1.workflows.delete(this.selectedWorkflowId);
        OpenFangToast.success('Workflow deleted');
        this.createWorkflowDraft();
        window.dispatchEvent(new CustomEvent('openfang:workflows-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to delete workflow: ' + e.message);
      }

      this.deletingId = '';
    },

    async validateWorkflow() {
      var definition;

      this.validating = true;
      this.validationResult = null;

      try {
        definition = this.buildDefinitionOrToast();
        this.validationResult = await OpenFangAPI.v1.workflows.validate({
          definition: definition,
          strict: false
        });
        if (this.validationResult.valid) {
          OpenFangToast.success('Workflow definition is valid');
        } else {
          OpenFangToast.warn('Workflow definition has validation issues');
        }
      } catch (e) {
        if (!e || !e.workflowBuildError) {
          OpenFangToast.error('Failed to validate workflow: ' + e.message);
        }
      }

      this.validating = false;
    },

    async compileWorkflow() {
      var definition;

      this.compiling = true;
      this.compileResult = null;

      try {
        definition = this.buildDefinitionOrToast();
        this.compileResult = await OpenFangAPI.v1.workflows.compile({
          definition: definition
        });
        OpenFangToast.success('Workflow compiled successfully');
      } catch (e) {
        if (!e || !e.workflowBuildError) {
          OpenFangToast.error('Failed to compile workflow: ' + e.message);
        }
      }

      this.compiling = false;
    },

    async forkSelectedWorkflow() {
      var forked;

      if (!this.selectedWorkflowId) return;
      this.forkingId = this.selectedWorkflowId;

      try {
        forked = await OpenFangAPI.v1.workflows.fork(this.selectedWorkflowId, {
          mode: 'shadow'
        });
        OpenFangToast.success('Workflow forked: ' + forked.name);
        window.dispatchEvent(new CustomEvent('openfang:workflows-changed'));
        await this.selectWorkflow(forked.id, true);
      } catch (e) {
        OpenFangToast.error('Failed to fork workflow: ' + e.message);
      }

      this.forkingId = '';
    },

    runFieldInputType(field) {
      if (field.kind === 'integer' || field.kind === 'number') return 'number';
      if (
        field.kind === 'artifact_ref' ||
        field.kind === 'doc_ref' ||
        field.kind === 'issue_ref' ||
        field.kind === 'task_ref' ||
        field.kind === 'task_list' ||
        field.kind === 'run_ref'
      ) {
        return 'text';
      }
      return 'text';
    },

    runFieldRequiresJson(field) {
      return field.kind === 'object' || field.kind === 'array' || field.kind === 'any';
    },

    runFieldIsBoolean(field) {
      return field.kind === 'boolean';
    },

    buildRunInputPayload() {
      var contract = this.currentInputContract;
      var fields = this.runInputFields;
      var values = this.runForm.values || {};
      var payload;
      var i;
      var rawValue;
      var parsedValue;
      var field;

      if (!contract) {
        throw new Error('Input contract JSON is invalid');
      }

      if (contract.kind !== 'object' || !fields.length || (fields.length === 1 && !fields[0].key)) {
        field = fields.length ? fields[0] : null;
        rawValue = field ? values[field.key] : workflowUiNormalizeText(values['']);
        if (field && this.runFieldRequiresJson(field)) {
          return workflowUiParseJson(rawValue, {});
        }
        if (field && field.kind === 'integer' && workflowUiNormalizeText(rawValue).trim()) {
          return Number(rawValue);
        }
        if (field && field.kind === 'number' && workflowUiNormalizeText(rawValue).trim()) {
          return Number(rawValue);
        }
        if (field && field.kind === 'boolean') {
          return !!rawValue;
        }
        return rawValue;
      }

      payload = {};

      for (i = 0; i < fields.length; i++) {
        field = fields[i];
        rawValue = values[field.key];

        if (field.kind === 'boolean') {
          parsedValue = !!rawValue;
        } else if (!workflowUiNormalizeText(rawValue).trim()) {
          if (field.required) {
            throw new Error('Missing required input field: ' + field.label);
          }
          continue;
        } else if (field.kind === 'integer' || field.kind === 'number') {
          parsedValue = Number(rawValue);
        } else if (this.runFieldRequiresJson(field)) {
          parsedValue = workflowUiParseJson(rawValue, field.kind === 'array' ? [] : {});
        } else {
          parsedValue = rawValue;
        }

        workflowUiSetNestedValue(payload, field.path, parsedValue);
      }

      return payload;
    },

    async startRun() {
      var requestBody;
      var runResult;

      if (!this.selectedWorkflowId) return;
      this.running = true;
      this.runResult = null;

      try {
        requestBody = {
          input: this.buildRunInputPayload(),
          labels: workflowUiNormalizeText(this.runForm.labels_text)
            .split(',')
            .map(function(label) { return label.trim(); })
            .filter(Boolean),
          metadata: workflowUiParseJson(this.runForm.metadata_text, {})
        };
        runResult = await OpenFangAPI.v1.workflows.startRun(this.selectedWorkflowId, requestBody);
        OpenFangToast.success('Workflow run accepted');
        await this.selectWorkflow(this.selectedWorkflowId, true);
        this.runResult = runResult;
      } catch (e) {
        OpenFangToast.error('Failed to start workflow run: ' + e.message);
      }

      this.running = false;
    },

    async dryRunWorkflow() {
      var requestBody;

      if (!this.selectedWorkflowId) return;
      this.dryRunning = true;
      this.dryRunResult = null;

      try {
        requestBody = {
          input: this.buildRunInputPayload(),
          labels: workflowUiNormalizeText(this.runForm.labels_text)
            .split(',')
            .map(function(label) { return label.trim(); })
            .filter(Boolean),
          metadata: workflowUiParseJson(this.runForm.metadata_text, {})
        };
        this.dryRunResult = await OpenFangAPI.v1.workflows.dryRun(this.selectedWorkflowId, requestBody);
        OpenFangToast.success('Dry-run preview ready');
      } catch (e) {
        OpenFangToast.error('Failed to run dry-run preview: ' + e.message);
      }

      this.dryRunning = false;
    },

    workflowOriginLabel(origin) {
      return workflowUiOriginLabel(origin);
    },

    workflowStepKindLabel(kind) {
      return workflowUiTitleCase(kind);
    },

    workflowLastRunText(item) {
      var runtime = this.runtimeSnapshot(item);
      if (!runtime.last_run_at) return 'Never';
      return OpenFangUtils.formatDateTime(runtime.last_run_at);
    }
  };
}
