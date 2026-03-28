// OpenFang Tasks Page — durable task and subtask control plane with replanning
'use strict';

function tasksPageDefaultTaskForm() {
  return {
    title: '',
    description: '',
    priority: 'medium',
    complexity: 'medium',
    ownerKind: 'agent_group',
    ownerRef: 'sdlc',
    slug: ''
  };
}

function tasksPageDefaultSubtaskForm() {
  return {
    id: '',
    title: '',
    description: '',
    kind: 'doc_change',
    status: 'planned',
    complexity: 'medium',
    position: '',
    assigneeKind: 'agent',
    assigneeRef: '',
    dependsOnText: '',
    parallelizable: false
  };
}

function tasksPageDefaultReplanCreateItem() {
  return {
    id: '',
    title: '',
    description: '',
    kind: 'doc_change',
    status: 'planned',
    complexity: 'medium',
    position: '',
    assigneeKind: 'agent',
    assigneeRef: '',
    dependsOnText: '',
    parallelizable: false
  };
}

function tasksPageDefaultReplanUpdateItem() {
  return {
    id: '',
    title: '',
    description: '',
    kind: '',
    status: '',
    complexity: '',
    position: '',
    assigneeKind: 'agent',
    assigneeRef: '',
    clearAssignee: false,
    dependsOnText: '',
    clearDependencies: false,
    parallelizableMode: ''
  };
}

function tasksPageNormalizeText(value) {
  return value === undefined || value === null ? '' : String(value);
}

function tasksPageSlugify(value) {
  var slug = tasksPageNormalizeText(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

  if (slug) {
    return slug;
  }

  return 'task-' + Date.now();
}

function tasksPageParseIdList(value) {
  return tasksPageNormalizeText(value)
    .split(/[\s,]+/)
    .map(function(part) {
      return part.trim();
    })
    .filter(function(part) {
      return part.length > 0;
    });
}

function tasksPageSortByPosition(left, right) {
  var leftPosition = left && left.position !== undefined ? Number(left.position) : Number.MAX_SAFE_INTEGER;
  var rightPosition = right && right.position !== undefined ? Number(right.position) : Number.MAX_SAFE_INTEGER;

  if (leftPosition !== rightPosition) {
    return leftPosition - rightPosition;
  }

  var leftUpdated = Date.parse(left && left.updated_at ? left.updated_at : '');
  var rightUpdated = Date.parse(right && right.updated_at ? right.updated_at : '');

  if (!isNaN(leftUpdated) && !isNaN(rightUpdated) && leftUpdated !== rightUpdated) {
    return leftUpdated - rightUpdated;
  }

  return tasksPageNormalizeText(left && left.title).localeCompare(tasksPageNormalizeText(right && right.title));
}

function tasksPage() {
  return {
    tasks: [],
    taskDetails: {},
    subtasks: [],
    subtaskDetails: {},
    artifacts: [],
    docs: [],
    files: [],
    loading: true,
    loadError: '',
    detailLoading: false,
    detailError: '',
    filterStatus: '',
    filterPriority: '',
    searchQuery: '',
    selectedTaskId: '',
    selectedTask: null,
    selectedTab: 'subtasks',
    showCreateModal: false,
    showEditModal: false,
    showSubtaskModal: false,
    showReplanModal: false,
    taskSubmitting: false,
    taskDeletingId: '',
    subtaskSubmitting: false,
    subtaskActionId: '',
    replanSubmitting: false,
    editingSubtaskId: '',
    createForm: tasksPageDefaultTaskForm(),
    editForm: tasksPageDefaultTaskForm(),
    subtaskForm: tasksPageDefaultSubtaskForm(),
    replanForm: {
      reason: '',
      cancelSubtaskIds: '',
      createItems: [],
      updateItems: []
    },
    replanResult: null,
    listRequestToken: 0,
    detailRequestToken: 0,
    nowTick: Date.now(),
    clockTimer: null,

    init() {
      var self = this;
      this.clockTimer = setInterval(function() {
        self.nowTick = Date.now();
      }, 30000);
      this.resetReplanForm();
      this.loadData();
    },

    destroy() {
      if (this.clockTimer) {
        clearInterval(this.clockTimer);
        this.clockTimer = null;
      }
    },

    get taskStatusOptions() {
      return [
        { value: '', label: 'All Statuses' },
        { value: 'planned', label: 'Planned' },
        { value: 'in_progress', label: 'In Progress' },
        { value: 'completed', label: 'Completed' },
        { value: 'failed', label: 'Failed' },
        { value: 'cancelled', label: 'Cancelled' }
      ];
    },

    get priorityOptions() {
      return [
        { value: '', label: 'All Priorities' },
        { value: 'low', label: 'Low' },
        { value: 'medium', label: 'Medium' },
        { value: 'high', label: 'High' },
        { value: 'critical', label: 'Critical' }
      ];
    },

    get editTaskStatusOptions() {
      return this.taskStatusOptions.slice(1);
    },

    get complexityOptions() {
      return [
        { value: 'low', label: 'Low' },
        { value: 'medium', label: 'Medium' },
        { value: 'high', label: 'High' }
      ];
    },

    get ownerKindOptions() {
      return [
        { value: 'agent_group', label: 'Agent Group' },
        { value: 'agent', label: 'Agent' },
        { value: 'user', label: 'User' }
      ];
    },

    get subtaskKindOptions() {
      return [
        { value: 'doc_change', label: 'Doc Change' },
        { value: 'code_change', label: 'Code Change' },
        { value: 'research', label: 'Research' },
        { value: 'review_item', label: 'Review Item' },
        { value: 'validation', label: 'Validation' },
        { value: 'other', label: 'Other' }
      ];
    },

    get subtaskStatusOptions() {
      return [
        { value: 'planned', label: 'Planned' },
        { value: 'ready', label: 'Ready' },
        { value: 'in_progress', label: 'In Progress' },
        { value: 'completed', label: 'Completed' },
        { value: 'failed', label: 'Failed' },
        { value: 'cancelled', label: 'Cancelled' }
      ];
    },

    get selectedTaskSubtaskRows() {
      return this.buildSubtaskRows(this.subtasks);
    },

    get selectedTaskRepositoryRefs() {
      if (!this.selectedTask || !Array.isArray(this.selectedTask.repository_refs)) {
        return [];
      }
      return this.selectedTask.repository_refs;
    },

    get selectedTaskLabelRefs() {
      if (!this.selectedTask || !Array.isArray(this.selectedTask.label_refs)) {
        return [];
      }
      return this.selectedTask.label_refs;
    },

    get lastReplanMetadata() {
      if (!this.selectedTask || !this.selectedTask.metadata || !this.selectedTask.metadata.last_replan) {
        return null;
      }
      return this.selectedTask.metadata.last_replan;
    },

    loadData() {
      return this.loadTasks({ refreshDetail: true });
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    taskQueryParams() {
      return {
        limit: 200,
        status: this.filterStatus || undefined,
        priority: this.filterPriority || undefined,
        q: this.searchQuery.trim() || undefined
      };
    },

    subtaskQueryParams() {
      return {
        limit: 200
      };
    },

    mergeTaskRecord(summary) {
      if (!summary || !summary.id) return summary;
      var detail = this.taskDetails[summary.id] || {};
      return Object.assign({}, summary, detail);
    },

    mergeSubtaskRecord(summary) {
      if (!summary || !summary.id) return summary;
      var detail = this.subtaskDetails[summary.id] || {};
      return Object.assign({}, summary, detail);
    },

    sortTasks(items) {
      return (items || []).slice().sort(tasksPageSortByPosition);
    },

    sortSubtasks(items) {
      return (items || []).slice().sort(tasksPageSortByPosition);
    },

    async loadTasks(options) {
      options = options || {};
      var requestToken = ++this.listRequestToken;

      if (!options.silent) {
        this.loading = true;
      }

      this.loadError = '';

      try {
        var response = await OpenFangAPI.v1.tasks.list(this.taskQueryParams());
        if (requestToken !== this.listRequestToken) return;

        var summaries = this.sortTasks(this.normalizeList(response));
        await this.hydrateTaskList(summaries, requestToken);

        if (requestToken !== this.listRequestToken) return;

        var items = this.sortTasks(summaries.map(
          function(summary) {
            return this.mergeTaskRecord(summary);
          },
          this
        ));
        this.tasks = items;

        var nextSelectedTaskId = this.resolveSelectedTaskId(items);
        if (!nextSelectedTaskId) {
          this.clearSelection();
        } else if (nextSelectedTaskId !== this.selectedTaskId) {
          await this.selectTask(nextSelectedTaskId, true);
        } else if (options.refreshDetail !== false && this.selectedTaskId) {
          await this.refreshSelectedTask();
        }
      } catch (e) {
        if (requestToken !== this.listRequestToken) return;
        this.tasks = [];
        this.loadError = e.message || 'Could not load tasks.';
        this.clearSelection();
      }

      if (!options.silent) {
        this.loading = false;
      }
    },

    async hydrateTaskList(summaries, requestToken) {
      var self = this;
      var detailPromises = (summaries || []).map(async function(summary) {
        try {
          return await self.getTaskDetail(summary.id);
        } catch (e) {
          return null;
        }
      });

      var details = await Promise.all(detailPromises);
      if (requestToken !== this.listRequestToken) return;

      for (var i = 0; i < details.length; i++) {
        var detail = details[i];
        if (!detail || !detail.id) continue;
        this.taskDetails[detail.id] = detail;
      }
    },

    resolveSelectedTaskId(items) {
      if (!items.length) return '';

      for (var i = 0; i < items.length; i++) {
        if (items[i].id === this.selectedTaskId) {
          return this.selectedTaskId;
        }
      }

      return items[0].id;
    },

    clearSelection() {
      this.selectedTaskId = '';
      this.selectedTask = null;
      this.subtasks = [];
      this.artifacts = [];
      this.docs = [];
      this.files = [];
      this.replanResult = null;
      this.detailError = '';
      this.detailLoading = false;
    },

    async selectTask(taskId, keepTab) {
      if (!taskId) {
        this.clearSelection();
        return;
      }

      this.selectedTaskId = taskId;
      this.selectedTask = this.taskDetails[taskId] || null;
      this.replanResult = null;

      if (!keepTab) {
        this.selectedTab = 'subtasks';
      }

      await this.refreshSelectedTask();
    },

    async refreshSelectedTask(options) {
      options = options || {};
      var taskId = this.selectedTaskId;
      if (!taskId) return;

      var requestToken = ++this.detailRequestToken;
      this.detailLoading = true;
      this.detailError = '';

      try {
        var responses = await Promise.all([
          this.getTaskDetail(taskId, !!options.force),
          OpenFangAPI.v1.tasks.subtasks(taskId, this.subtaskQueryParams()),
          OpenFangAPI.v1.tasks.artifacts(taskId),
          OpenFangAPI.v1.tasks.docs(taskId),
          OpenFangAPI.v1.tasks.files(taskId)
        ]);

        if (requestToken !== this.detailRequestToken || this.selectedTaskId !== taskId) {
          return;
        }

        var task = responses[0];
        var subtaskSummaries = this.normalizeList(responses[1]);
        var hydratedSubtasks = await this.hydrateSubtasks(subtaskSummaries);

        if (requestToken !== this.detailRequestToken || this.selectedTaskId !== taskId) {
          return;
        }

        this.selectedTask = task;
        this.artifacts = this.normalizeList(responses[2]);
        this.docs = this.normalizeList(responses[3]);
        this.files = this.normalizeList(responses[4]);
        this.subtasks = this.sortSubtasks(hydratedSubtasks);

        this.taskDetails[taskId] = task;
        this.tasks = this.sortTasks(this.tasks.map(
          function(item) {
            return item.id === taskId ? this.mergeTaskRecord(task) : this.mergeTaskRecord(item);
          },
          this
        ));
      } catch (e) {
        if (requestToken !== this.detailRequestToken || this.selectedTaskId !== taskId) {
          return;
        }
        this.detailError = e.message || 'Could not load task detail.';
      }

      this.detailLoading = false;
    },

    async hydrateSubtasks(summaries) {
      var self = this;
      var detailPromises = (summaries || []).map(async function(summary) {
        try {
          return await self.getSubtaskDetail(summary.id);
        } catch (e) {
          return self.mergeSubtaskRecord(summary);
        }
      });
      return Promise.all(detailPromises);
    },

    async getTaskDetail(taskId, force) {
      if (!force && this.taskDetails[taskId]) {
        return this.taskDetails[taskId];
      }

      var detail = await OpenFangAPI.v1.tasks.get(taskId);
      this.taskDetails[taskId] = detail;
      return detail;
    },

    async getSubtaskDetail(subtaskId, force) {
      if (!force && this.subtaskDetails[subtaskId]) {
        return this.subtaskDetails[subtaskId];
      }

      var detail = await OpenFangAPI.v1.subtasks.get(subtaskId);
      this.subtaskDetails[subtaskId] = detail;
      return detail;
    },

    setTab(tab) {
      this.selectedTab = tab;
    },

    openCreateModal() {
      this.createForm = tasksPageDefaultTaskForm();
      this.showCreateModal = true;
    },

    closeCreateModal() {
      this.showCreateModal = false;
    },

    openEditModal() {
      if (!this.selectedTask) return;
      this.editForm = {
        title: this.selectedTask.title || '',
        description: this.selectedTask.description || '',
        priority: this.selectedTask.priority || 'medium',
        complexity: this.selectedTask.complexity || 'medium',
        ownerKind: this.selectedTask.owner ? this.selectedTask.owner.kind : 'agent_group',
        ownerRef: this.selectedTask.owner ? this.selectedTask.owner.ref : '',
        slug: this.selectedTask.slug || '',
        status: this.selectedTask.status || 'planned'
      };
      this.showEditModal = true;
    },

    closeEditModal() {
      this.showEditModal = false;
    },

    resetSubtaskForm() {
      this.subtaskForm = tasksPageDefaultSubtaskForm();
      this.editingSubtaskId = '';
    },

    openCreateSubtaskModal() {
      if (!this.selectedTaskId) return;
      this.resetSubtaskForm();
      this.subtaskForm.position = String(this.nextSubtaskPosition());
      this.showSubtaskModal = true;
    },

    openEditSubtaskModal(subtask) {
      if (!subtask) return;
      this.editingSubtaskId = subtask.id;
      this.subtaskForm = {
        id: subtask.id || '',
        title: subtask.title || '',
        description: subtask.description || '',
        kind: subtask.kind || 'doc_change',
        status: subtask.status || 'planned',
        complexity: subtask.complexity || 'medium',
        position: subtask.position !== undefined ? String(subtask.position) : '',
        assigneeKind: subtask.assignee ? subtask.assignee.kind : 'agent',
        assigneeRef: subtask.assignee ? subtask.assignee.ref : '',
        dependsOnText: Array.isArray(subtask.depends_on) ? subtask.depends_on.join('\n') : '',
        parallelizable: !!subtask.parallelizable
      };
      this.showSubtaskModal = true;
    },

    closeSubtaskModal() {
      this.showSubtaskModal = false;
      this.resetSubtaskForm();
    },

    openReplanModal() {
      if (!this.selectedTaskId) return;
      this.resetReplanForm();
      this.showReplanModal = true;
    },

    closeReplanModal() {
      this.showReplanModal = false;
    },

    resetReplanForm() {
      this.replanForm = {
        reason: '',
        cancelSubtaskIds: '',
        createItems: [tasksPageDefaultReplanCreateItem()],
        updateItems: [tasksPageDefaultReplanUpdateItem()]
      };
    },

    addReplanCreateItem() {
      this.replanForm.createItems.push(tasksPageDefaultReplanCreateItem());
    },

    removeReplanCreateItem(index) {
      this.replanForm.createItems.splice(index, 1);
      if (!this.replanForm.createItems.length) {
        this.replanForm.createItems.push(tasksPageDefaultReplanCreateItem());
      }
    },

    addReplanUpdateItem() {
      this.replanForm.updateItems.push(tasksPageDefaultReplanUpdateItem());
    },

    removeReplanUpdateItem(index) {
      this.replanForm.updateItems.splice(index, 1);
      if (!this.replanForm.updateItems.length) {
        this.replanForm.updateItems.push(tasksPageDefaultReplanUpdateItem());
      }
    },

    nextTaskPosition() {
      var maxPosition = 0;
      for (var i = 0; i < this.tasks.length; i++) {
        var current = Number(this.tasks[i].position);
        if (!isNaN(current)) {
          maxPosition = Math.max(maxPosition, current);
        }
      }
      return maxPosition + 1;
    },

    nextSubtaskPosition() {
      var maxPosition = 0;
      for (var i = 0; i < this.subtasks.length; i++) {
        var current = Number(this.subtasks[i].position);
        if (!isNaN(current)) {
          maxPosition = Math.max(maxPosition, current);
        }
      }
      return maxPosition + 1;
    },

    buildTaskCreatePayload() {
      var slug = tasksPageNormalizeText(this.createForm.slug).trim() || tasksPageSlugify(this.createForm.title);

      return {
        slug: slug,
        title: this.createForm.title.trim(),
        description: this.createForm.description.trim(),
        status: 'planned',
        priority: this.createForm.priority,
        complexity: this.createForm.complexity,
        position: this.nextTaskPosition(),
        source: { kind: 'manual' },
        owner: {
          kind: this.createForm.ownerKind,
          ref: this.createForm.ownerRef.trim()
        },
        created_by: {
          kind: 'user',
          ref: 'dashboard'
        },
        repository_refs: [],
        label_refs: [],
        artifact_refs: [],
        doc_refs: [],
        file_refs: [],
        metadata: {
          source: 'dashboard',
          ui: 'tasks'
        }
      };
    },

    buildTaskUpdatePayload() {
      return {
        slug: tasksPageNormalizeText(this.editForm.slug).trim() || undefined,
        title: this.editForm.title.trim(),
        description: this.editForm.description.trim(),
        status: this.editForm.status,
        priority: this.editForm.priority,
        complexity: this.editForm.complexity,
        owner: {
          kind: this.editForm.ownerKind,
          ref: this.editForm.ownerRef.trim()
        }
      };
    },

    buildSubtaskPayload() {
      var assigneeRef = this.subtaskForm.assigneeRef.trim();
      var parsedPosition = parseInt(this.subtaskForm.position, 10);

      return {
        id: this.subtaskForm.id.trim() || undefined,
        title: this.subtaskForm.title.trim(),
        description: this.subtaskForm.description.trim(),
        kind: this.subtaskForm.kind,
        status: this.subtaskForm.status,
        complexity: this.subtaskForm.complexity,
        position: isNaN(parsedPosition) ? this.nextSubtaskPosition() : parsedPosition,
        assignee: assigneeRef ? {
          kind: this.subtaskForm.assigneeKind,
          ref: assigneeRef
        } : undefined,
        depends_on: tasksPageParseIdList(this.subtaskForm.dependsOnText),
        parallelizable: !!this.subtaskForm.parallelizable,
        input: {},
        metadata: {
          source: 'dashboard'
        }
      };
    },

    buildSubtaskUpdatePayload() {
      var assigneeRef = this.subtaskForm.assigneeRef.trim();
      var parsedPosition = parseInt(this.subtaskForm.position, 10);

      return {
        title: this.subtaskForm.title.trim(),
        description: this.subtaskForm.description.trim(),
        kind: this.subtaskForm.kind,
        status: this.subtaskForm.status,
        complexity: this.subtaskForm.complexity,
        position: isNaN(parsedPosition) ? undefined : parsedPosition,
        assignee: assigneeRef ? {
          kind: this.subtaskForm.assigneeKind,
          ref: assigneeRef
        } : null,
        depends_on: tasksPageParseIdList(this.subtaskForm.dependsOnText),
        parallelizable: !!this.subtaskForm.parallelizable
      };
    },

    buildReplanCreateItemPayload(item, index) {
      var title = tasksPageNormalizeText(item.title).trim();
      if (!title) return null;

      var parsedPosition = parseInt(item.position, 10);
      var assigneeRef = tasksPageNormalizeText(item.assigneeRef).trim();

      return {
        id: tasksPageNormalizeText(item.id).trim() || undefined,
        title: title,
        description: tasksPageNormalizeText(item.description).trim(),
        kind: item.kind || 'doc_change',
        status: item.status || 'planned',
        complexity: item.complexity || 'medium',
        position: isNaN(parsedPosition) ? this.nextSubtaskPosition() + index : parsedPosition,
        assignee: assigneeRef ? {
          kind: item.assigneeKind || 'agent',
          ref: assigneeRef
        } : undefined,
        depends_on: tasksPageParseIdList(item.dependsOnText),
        parallelizable: !!item.parallelizable,
        input: {},
        metadata: {
          source: 'dashboard',
          kind: 'replan'
        }
      };
    },

    buildReplanUpdateItemPayload(item) {
      var id = tasksPageNormalizeText(item.id).trim();
      if (!id) return null;

      var payload = { id: id };
      var hasChanges = false;

      if (tasksPageNormalizeText(item.title).trim()) {
        payload.title = item.title.trim();
        hasChanges = true;
      }

      if (tasksPageNormalizeText(item.description).trim()) {
        payload.description = item.description.trim();
        hasChanges = true;
      }

      if (item.kind) {
        payload.kind = item.kind;
        hasChanges = true;
      }

      if (item.status) {
        payload.status = item.status;
        hasChanges = true;
      }

      if (item.complexity) {
        payload.complexity = item.complexity;
        hasChanges = true;
      }

      if (tasksPageNormalizeText(item.position).trim()) {
        var parsedPosition = parseInt(item.position, 10);
        if (!isNaN(parsedPosition)) {
          payload.position = parsedPosition;
          hasChanges = true;
        }
      }

      if (item.clearAssignee) {
        payload.assignee = null;
        hasChanges = true;
      } else if (tasksPageNormalizeText(item.assigneeRef).trim()) {
        payload.assignee = {
          kind: item.assigneeKind || 'agent',
          ref: item.assigneeRef.trim()
        };
        hasChanges = true;
      }

      if (item.clearDependencies) {
        payload.depends_on = [];
        hasChanges = true;
      } else if (tasksPageNormalizeText(item.dependsOnText).trim()) {
        payload.depends_on = tasksPageParseIdList(item.dependsOnText);
        hasChanges = true;
      }

      if (item.parallelizableMode === 'true') {
        payload.parallelizable = true;
        hasChanges = true;
      } else if (item.parallelizableMode === 'false') {
        payload.parallelizable = false;
        hasChanges = true;
      }

      return hasChanges ? payload : null;
    },

    buildReplanPayload() {
      var operations = [];
      var cancelIds = tasksPageParseIdList(this.replanForm.cancelSubtaskIds);

      if (cancelIds.length) {
        operations.push({
          op: 'cancel_subtasks',
          subtask_ids: cancelIds
        });
      }

      var createItems = [];
      for (var i = 0; i < this.replanForm.createItems.length; i++) {
        var created = this.buildReplanCreateItemPayload(this.replanForm.createItems[i], i);
        if (created) {
          createItems.push(created);
        }
      }

      if (createItems.length) {
        operations.push({
          op: 'create_subtasks',
          items: createItems
        });
      }

      var updateItems = [];
      for (var j = 0; j < this.replanForm.updateItems.length; j++) {
        var updated = this.buildReplanUpdateItemPayload(this.replanForm.updateItems[j]);
        if (updated) {
          updateItems.push(updated);
        }
      }

      if (updateItems.length) {
        operations.push({
          op: 'update_subtasks',
          items: updateItems
        });
      }

      return {
        reason: this.replanForm.reason.trim(),
        operations: operations,
        metadata: {
          source: 'dashboard'
        }
      };
    },

    async createTask() {
      if (!this.createForm.title.trim()) {
        OpenFangToast.error('Task title is required.');
        return;
      }

      if (!this.createForm.ownerRef.trim()) {
        OpenFangToast.error('Task owner is required.');
        return;
      }

      this.taskSubmitting = true;

      try {
        var created = await OpenFangAPI.v1.tasks.create(this.buildTaskCreatePayload());
        this.taskDetails[created.id] = created;
        this.closeCreateModal();

        if (this.filterStatus && this.filterStatus !== created.status) {
          this.filterStatus = '';
        }
        if (this.filterPriority && this.filterPriority !== created.priority) {
          this.filterPriority = '';
        }
        this.searchQuery = '';

        await this.loadTasks({ refreshDetail: false, silent: true });
        await this.selectTask(created.id, false);
        OpenFangToast.success('Task created.');
      } catch (e) {
        OpenFangToast.error('Failed to create task: ' + (e.message || 'unknown error'));
      }

      this.taskSubmitting = false;
    },

    async saveTask() {
      if (!this.selectedTaskId) return;

      if (!this.editForm.title.trim()) {
        OpenFangToast.error('Task title is required.');
        return;
      }

      if (!this.editForm.ownerRef.trim()) {
        OpenFangToast.error('Task owner is required.');
        return;
      }

      this.taskSubmitting = true;

      try {
        var updated = await OpenFangAPI.v1.tasks.update(
          this.selectedTaskId,
          this.buildTaskUpdatePayload()
        );
        this.taskDetails[updated.id] = updated;
        this.selectedTask = updated;
        this.closeEditModal();
        await this.loadTasks({ refreshDetail: false, silent: true });
        await this.refreshSelectedTask({ force: true });
        OpenFangToast.success('Task updated.');
      } catch (e) {
        OpenFangToast.error('Failed to update task: ' + (e.message || 'unknown error'));
      }

      this.taskSubmitting = false;
    },

    confirmDeleteTask(task) {
      if (!task) return;

      var self = this;
      OpenFangUtils.confirmAction(
        'Delete Task',
        'Delete "' + (task.title || task.id) + '" and remove it from the durable task list? This cannot be undone.',
        async function() {
          self.taskDeletingId = task.id;
          try {
            await OpenFangAPI.v1.tasks.delete(task.id);
            delete self.taskDetails[task.id];
            if (self.selectedTaskId === task.id) {
              self.clearSelection();
            }
            await self.loadTasks({ refreshDetail: false, silent: true });
            OpenFangToast.success('Task deleted.');
          } catch (e) {
            OpenFangToast.error('Failed to delete task: ' + (e.message || 'unknown error'));
          }
          self.taskDeletingId = '';
        }
      );
    },

    async saveSubtask() {
      if (!this.selectedTaskId) return;

      if (!this.subtaskForm.title.trim()) {
        OpenFangToast.error('Subtask title is required.');
        return;
      }

      this.subtaskSubmitting = true;

      try {
        if (this.editingSubtaskId) {
          await OpenFangAPI.v1.subtasks.update(
            this.editingSubtaskId,
            this.buildSubtaskUpdatePayload()
          );
          OpenFangToast.success('Subtask updated.');
        } else {
          await OpenFangAPI.v1.tasks.createSubtask(
            this.selectedTaskId,
            this.buildSubtaskPayload()
          );
          OpenFangToast.success('Subtask created.');
        }

        this.closeSubtaskModal();
        await this.refreshSelectedTask({ force: true });
      } catch (e) {
        OpenFangToast.error('Failed to save subtask: ' + (e.message || 'unknown error'));
      }

      this.subtaskSubmitting = false;
    },

    confirmDeleteSubtask(subtask) {
      if (!subtask) return;

      var self = this;
      OpenFangUtils.confirmAction(
        'Delete Subtask',
        'Delete "' + (subtask.title || subtask.id) + '" from this task? This cannot be undone.',
        async function() {
          self.subtaskActionId = 'delete:' + subtask.id;
          try {
            await OpenFangAPI.v1.subtasks.delete(subtask.id);
            delete self.subtaskDetails[subtask.id];
            await self.refreshSelectedTask({ force: true });
            OpenFangToast.success('Subtask deleted.');
          } catch (e) {
            OpenFangToast.error('Failed to delete subtask: ' + (e.message || 'unknown error'));
          }
          self.subtaskActionId = '';
        }
      );
    },

    async advanceSubtaskStatus(subtask) {
      if (!subtask) return;

      var nextStatus = this.nextSubtaskStatus(subtask);
      if (!nextStatus) return;

      this.subtaskActionId = 'advance:' + subtask.id;

      try {
        await OpenFangAPI.v1.subtasks.update(subtask.id, { status: nextStatus });
        await this.refreshSelectedTask({ force: true });
        OpenFangToast.success('Subtask moved to ' + this.statusLabel(nextStatus) + '.');
      } catch (e) {
        OpenFangToast.error('Failed to update subtask status: ' + (e.message || 'unknown error'));
      }

      this.subtaskActionId = '';
    },

    async submitReplan() {
      if (!this.selectedTaskId) return;

      var payload = this.buildReplanPayload();
      if (!payload.reason) {
        OpenFangToast.error('A replan reason is required.');
        return;
      }

      if (!payload.operations.length) {
        OpenFangToast.error('Add at least one cancel, create, or update operation.');
        return;
      }

      this.replanSubmitting = true;

      try {
        var result = await OpenFangAPI.v1.tasks.replan(this.selectedTaskId, payload);
        this.replanResult = result;
        this.closeReplanModal();
        await this.refreshSelectedTask({ force: true });
        OpenFangToast.success('Task replan accepted.');
      } catch (e) {
        OpenFangToast.error('Failed to replan task: ' + (e.message || 'unknown error'));
      }

      this.replanSubmitting = false;
    },

    subtaskStatusBadgeClass(status) {
      return this.statusBadgeClass(status);
    },

    statusBadgeClass(status) {
      var normalized = tasksPageNormalizeText(status).toLowerCase();
      if (normalized === 'planned') return 'badge badge-task-planned';
      if (normalized === 'ready') return 'badge badge-task-ready';
      if (normalized === 'in_progress') return 'badge badge-task-in-progress';
      if (normalized === 'completed') return 'badge badge-task-completed';
      if (normalized === 'failed') return 'badge badge-task-failed';
      if (normalized === 'cancelled') return 'badge badge-task-cancelled';
      return 'badge badge-dim';
    },

    priorityBadgeClass(priority) {
      var normalized = tasksPageNormalizeText(priority).toLowerCase();
      if (normalized === 'critical') return 'badge badge-priority-critical';
      if (normalized === 'high') return 'badge badge-priority-high';
      if (normalized === 'medium') return 'badge badge-priority-medium';
      if (normalized === 'low') return 'badge badge-priority-low';
      return 'badge badge-dim';
    },

    complexityBadgeClass(complexity) {
      var normalized = tasksPageNormalizeText(complexity).toLowerCase();
      if (normalized === 'high') return 'badge badge-complexity-high';
      if (normalized === 'medium') return 'badge badge-complexity-medium';
      if (normalized === 'low') return 'badge badge-complexity-low';
      return 'badge badge-dim';
    },

    subtaskKindBadgeClass(kind) {
      var normalized = tasksPageNormalizeText(kind).toLowerCase();
      if (!normalized) return 'badge badge-dim';
      return 'badge badge-subtask-kind-' + normalized;
    },

    readinessBadgeClass(state) {
      if (state === 'blocked') return 'badge badge-readiness-blocked';
      if (state === 'ready') return 'badge badge-readiness-ready';
      return 'badge badge-dim';
    },

    readinessLabel(state) {
      if (state === 'blocked') return 'Blocked';
      if (state === 'ready') return 'Ready';
      return 'Waiting';
    },

    titleCase(value) {
      return tasksPageNormalizeText(value)
        .replace(/_/g, ' ')
        .replace(/\b\w/g, function(letter) {
          return letter.toUpperCase();
        });
    },

    statusLabel(status) {
      return this.titleCase(status || 'unknown');
    },

    priorityLabel(priority) {
      return this.titleCase(priority || 'unknown');
    },

    complexityLabel(complexity) {
      return this.titleCase(complexity || 'unknown');
    },

    ownerLabel(owner) {
      if (!owner) return 'Unknown';
      return tasksPageNormalizeText(owner.ref || owner.ref_id || '-') + ' · ' + this.titleCase(owner.kind || 'unknown');
    },

    sourceLabel(source) {
      if (!source || !source.kind) return 'Unknown';
      if (source.kind === 'workflow') {
        return 'Workflow';
      }
      return this.titleCase(source.kind);
    },

    sourceHint(source) {
      if (!source || !source.kind) return '';
      if (source.kind === 'workflow') {
        return tasksPageNormalizeText(source.workflow_id) + ' / ' + tasksPageNormalizeText(source.run_id);
      }
      return '';
    },

    subtaskAssigneeLabel(subtask) {
      if (!subtask || !subtask.assignee) return 'Unassigned';
      return tasksPageNormalizeText(subtask.assignee.ref || subtask.assignee.ref_id || '-') + ' · ' + this.titleCase(subtask.assignee.kind || 'unknown');
    },

    formatDateTime(value) {
      return OpenFangUtils.formatDateTime(value);
    },

    relativeTime(value) {
      return OpenFangUtils.timeAgo(value);
    },

    taskCreatedAt(task) {
      return task && task.created_at ? this.formatDateTime(task.created_at) : '-';
    },

    taskUpdatedAt(task) {
      return task && task.updated_at ? this.formatDateTime(task.updated_at) : '-';
    },

    copyTaskId(task) {
      if (!task || !task.id) return;
      OpenFangUtils.copyToClipboard(task.id);
    },

    resourceVersionLabel(resource) {
      if (!resource) return '-';
      return resource.current_version_id || '-';
    },

    dependencyRecords(subtask) {
      if (!subtask || !Array.isArray(subtask.depends_on)) return [];
      var records = [];

      for (var i = 0; i < subtask.depends_on.length; i++) {
        var dependencyId = subtask.depends_on[i];
        var dependency = this.subtaskDetails[dependencyId] || null;
        records.push({
          id: dependencyId,
          title: dependency ? dependency.title : dependencyId,
          status: dependency ? dependency.status : 'missing'
        });
      }

      return records;
    },

    isSubtaskBlocked(subtask) {
      if (!subtask || !Array.isArray(subtask.depends_on) || !subtask.depends_on.length) {
        return false;
      }

      for (var i = 0; i < subtask.depends_on.length; i++) {
        var dependency = this.subtaskDetails[subtask.depends_on[i]];
        if (!dependency || dependency.status !== 'completed') {
          return true;
        }
      }

      return false;
    },

    isSubtaskReady(subtask) {
      if (!subtask) return false;
      if (subtask.status === 'completed' || subtask.status === 'failed' || subtask.status === 'cancelled') {
        return false;
      }
      if (subtask.status === 'in_progress') {
        return false;
      }
      return !this.isSubtaskBlocked(subtask);
    },

    subtaskReadiness(subtask) {
      if (this.isSubtaskBlocked(subtask)) return 'blocked';
      if (this.isSubtaskReady(subtask)) return 'ready';
      return '';
    },

    nextSubtaskStatus(subtask) {
      if (!subtask) return '';
      if (subtask.status === 'planned' || subtask.status === 'ready') return 'in_progress';
      if (subtask.status === 'in_progress') return 'completed';
      return '';
    },

    nextSubtaskStatusLabel(subtask) {
      var next = this.nextSubtaskStatus(subtask);
      if (next === 'in_progress') return 'Start';
      if (next === 'completed') return 'Complete';
      return 'Locked';
    },

    buildSubtaskRows(items) {
      var sorted = this.sortSubtasks(items);
      var byId = {};
      var children = {};
      var visited = {};
      var rows = [];
      var i;

      for (i = 0; i < sorted.length; i++) {
        byId[sorted[i].id] = sorted[i];
        children[sorted[i].id] = [];
      }

      for (i = 0; i < sorted.length; i++) {
        var subtask = sorted[i];
        var primaryDependency = Array.isArray(subtask.depends_on) && subtask.depends_on.length
          ? subtask.depends_on[0]
          : '';
        if (primaryDependency && children[primaryDependency]) {
          children[primaryDependency].push(subtask);
        }
      }

      var visit = function(component, subtask, level) {
        if (!subtask || visited[subtask.id]) return;
        visited[subtask.id] = true;
        rows.push({
          key: subtask.id,
          subtask: subtask,
          level: level,
          readiness: component.subtaskReadiness(subtask),
          dependencies: component.dependencyRecords(subtask)
        });

        var nextChildren = component.sortSubtasks(children[subtask.id] || []);
        for (var childIndex = 0; childIndex < nextChildren.length; childIndex++) {
          visit(component, nextChildren[childIndex], level + 1);
        }
      };

      for (i = 0; i < sorted.length; i++) {
        var current = sorted[i];
        if (!current.depends_on || !current.depends_on.length || !byId[current.depends_on[0]]) {
          visit(this, current, 0);
        }
      }

      for (i = 0; i < sorted.length; i++) {
        visit(this, sorted[i], 0);
      }

      return rows;
    }
  };
}
