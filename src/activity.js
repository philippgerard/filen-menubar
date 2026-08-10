const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const ACTIVITY_LIMIT = 500;
const FILTERS = new Set(["all", "uploads", "downloads", "changes", "errors"]);
const CATEGORIES = new Set(["upload", "download", "change"]);
const OUTCOMES = new Set(["success", "failed"]);

let copy;
let entries = [];
let activeFilter = "all";
let refreshGeneration = 0;
let scheduledRefresh;
let stopListening;
let clearing = false;

const element = (id) => document.getElementById(id);

function applyCopy(values) {
  document.title = values.windowTitle;
  document.documentElement.lang = values.locale;
  document.documentElement.dataset.platform = values.platform;

  const text = {
    title: values.title,
    intro: values.intro,
    "search-label": values.searchLabel,
    "loading-label": values.loading,
    "load-failed-title": values.loadFailedTitle,
    "load-failed-description": values.loadFailedDescription,
    "retry-button": values.retryButton,
    "empty-title": values.emptyTitle,
    "empty-description": values.emptyDescription,
    "no-results-title": values.noResultsTitle,
    "no-results-description": values.noResultsDescription,
    "clear-button": values.clearButton,
    "clear-confirm-prompt": values.clearConfirmPrompt,
    "clear-confirm": values.clearConfirmButton,
    "clear-cancel": values.clearCancelButton,
  };

  for (const [id, value] of Object.entries(text)) {
    element(id).textContent = value;
  }

  element("activity-search").placeholder = values.searchPlaceholder;
  element("activity-filters").setAttribute("aria-label", values.filterLabel);
  element("activity-list").setAttribute("aria-label", values.listLabel);

  const filterText = {
    all: values.filterAll,
    uploads: values.filterUploads,
    downloads: values.filterDownloads,
    changes: values.filterChanges,
    errors: values.filterErrors,
  };

  for (const button of document.querySelectorAll("[data-filter]")) {
    button.textContent = filterText[button.dataset.filter];
  }
}

function normalizedEntry(raw) {
  if (!raw || !CATEGORIES.has(raw.category) || !OUTCOMES.has(raw.outcome)) {
    return null;
  }

  const observedAtMs = Number(raw.observedAtMs);
  const parsedSize = raw.size === null || raw.size === undefined ? null : Number(raw.size);

  return {
    id: raw.id,
    observedAtMs: Number.isFinite(observedAtMs) ? observedAtMs : null,
    category: raw.category,
    outcome: raw.outcome,
    action: typeof raw.action === "string" ? raw.action : "",
    relativePath: typeof raw.relativePath === "string" ? raw.relativePath : "",
    size: Number.isFinite(parsedSize) && parsedSize >= 0 ? parsedSize : null,
  };
}

function normalizeEntries(values) {
  if (!Array.isArray(values)) {
    return [];
  }

  return values
    .map(normalizedEntry)
    .filter(Boolean)
    .sort((left, right) => (right.observedAtMs ?? 0) - (left.observedAtMs ?? 0))
    .slice(0, ACTIVITY_LIMIT);
}

function localizedCategory(category) {
  return {
    upload: copy.categoryUpload,
    download: copy.categoryDownload,
    change: copy.categoryChange,
  }[category];
}

function localizedOutcome(outcome) {
  return outcome === "failed" ? copy.outcomeFailed : copy.outcomeSuccess;
}

function formatTimestamp(observedAtMs) {
  if (observedAtMs === null) {
    return "";
  }

  const date = new Date(observedAtMs);
  if (Number.isNaN(date.getTime())) {
    return "";
  }

  return new Intl.DateTimeFormat(copy.locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function formatSize(size) {
  if (size === null) {
    return "";
  }

  const units = [copy.sizeB, copy.sizeKb, copy.sizeMb, copy.sizeGb, copy.sizeTb];
  let value = size;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const formatted = new Intl.NumberFormat(copy.locale, {
    maximumFractionDigits: unitIndex === 0 ? 0 : 1,
  }).format(value);

  return `${formatted}\u00a0${units[unitIndex]}`;
}

function interpolateCount(count) {
  const template = count === 1 ? copy.countOne : copy.countMany;
  return template
    .split("%{count}")
    .join(new Intl.NumberFormat(copy.locale).format(count));
}

function iconFor(category) {
  const namespace = "http://www.w3.org/2000/svg";
  const wrapper = document.createElement("span");
  const icon = document.createElementNS(namespace, "svg");
  const path = document.createElementNS(namespace, "path");

  wrapper.className = `category-icon ${category}`;
  wrapper.setAttribute("aria-hidden", "true");
  icon.setAttribute("viewBox", "0 0 20 20");

  const pathData = {
    upload: "M10 15.5v-11m-4 4 4-4 4 4M5 16.5h10",
    download: "M10 4.5v11m-4-4 4 4 4-4M5 3.5h10",
    change: "M5 6.5h8.5m-2.5-2.5 2.5 2.5-2.5 2.5M15 13.5H6.5M9 11l-2.5 2.5L9 16",
  };

  path.setAttribute("d", pathData[category]);
  icon.append(path);
  wrapper.append(icon);
  return wrapper;
}

function metadataItem(value) {
  const item = document.createElement("span");
  item.className = "metadata-item";
  item.textContent = value;
  return item;
}

function rowFor(entry) {
  const row = document.createElement("li");
  const content = document.createElement("div");
  const title = document.createElement("p");
  const path = document.createElement("p");
  const metadata = document.createElement("div");
  const outcome = document.createElement("span");

  row.className = "activity-row";
  if (entry.id !== null && entry.id !== undefined) {
    row.dataset.entryId = String(entry.id);
  }

  content.className = "operation-copy";
  title.className = "operation-title";
  title.textContent = entry.action;
  path.className = "operation-path";
  path.textContent = entry.relativePath;
  path.title = entry.relativePath;

  metadata.className = "operation-metadata";
  metadata.append(metadataItem(localizedCategory(entry.category)));

  const timestamp = formatTimestamp(entry.observedAtMs);
  if (timestamp) {
    const time = document.createElement("time");
    time.className = "metadata-item";
    time.dateTime = new Date(entry.observedAtMs).toISOString();
    time.textContent = timestamp;
    metadata.append(time);
  }

  const size = formatSize(entry.size);
  if (size) {
    metadata.append(metadataItem(size));
  }

  outcome.className = `outcome-badge ${entry.outcome}`;
  outcome.textContent = localizedOutcome(entry.outcome);

  content.append(title);
  if (entry.relativePath) {
    content.append(path);
  }
  content.append(metadata);
  row.append(iconFor(entry.category), content, outcome);
  return row;
}

function matchesFilter(entry) {
  switch (activeFilter) {
    case "uploads":
      return entry.category === "upload";
    case "downloads":
      return entry.category === "download";
    case "changes":
      return entry.category === "change";
    case "errors":
      return entry.outcome === "failed";
    default:
      return true;
  }
}

function matchesSearch(entry, query) {
  if (!query) {
    return true;
  }

  const searchable = [
    entry.action,
    entry.relativePath,
    localizedCategory(entry.category),
    localizedOutcome(entry.outcome),
  ]
    .join("\n")
    .toLocaleLowerCase();

  return searchable.includes(query);
}

function visibleEntries() {
  const query = element("activity-search").value.trim().toLocaleLowerCase();
  return entries.filter((entry) => matchesFilter(entry) && matchesSearch(entry, query));
}

function showContent(name) {
  const views = {
    loading: element("loading-state"),
    error: element("error-state"),
    empty: element("empty-state"),
    noResults: element("no-results-state"),
    list: element("activity-list"),
  };

  for (const [viewName, view] of Object.entries(views)) {
    view.hidden = viewName !== name;
  }

  element("activity-content").setAttribute("aria-busy", String(name === "loading"));
}

function updateFooter(count) {
  element("activity-count").textContent = interpolateCount(count);
  element("clear-button").disabled = entries.length === 0 || clearing;

  if (entries.length === 0) {
    setClearConfirmation(false);
  }
}

function render() {
  if (entries.length === 0) {
    element("activity-list").replaceChildren();
    showContent("empty");
    updateFooter(0);
    return;
  }

  const visible = visibleEntries();
  updateFooter(visible.length);

  if (visible.length === 0) {
    element("activity-list").replaceChildren();
    showContent("noResults");
    return;
  }

  const fragment = document.createDocumentFragment();
  for (const entry of visible) {
    fragment.append(rowFor(entry));
  }

  element("activity-list").replaceChildren(fragment);
  showContent("list");
}

async function refresh({ showLoading = false } = {}) {
  const generation = ++refreshGeneration;

  if (showLoading) {
    showContent("loading");
    element("activity-count").textContent = "";
  }

  try {
    const recent = await invoke("recent_activity");
    if (generation !== refreshGeneration) {
      return;
    }

    entries = normalizeEntries(recent);
    render();
  } catch (error) {
    if (generation !== refreshGeneration) {
      return;
    }

    console.error(error);
    showContent("error");
    element("activity-count").textContent = "";
    element("clear-button").disabled = true;
  }
}

function scheduleRefresh() {
  if (scheduledRefresh !== undefined) {
    return;
  }

  scheduledRefresh = window.setTimeout(() => {
    scheduledRefresh = undefined;
    void refresh();
  }, 80);
}

function selectFilter(filter, { focus = false } = {}) {
  if (!FILTERS.has(filter)) {
    return;
  }

  activeFilter = filter;
  for (const button of document.querySelectorAll("[data-filter]")) {
    const selected = button.dataset.filter === filter;
    button.setAttribute("aria-pressed", String(selected));
    if (selected && focus) {
      button.focus();
    }
  }
  render();
}

function setClearConfirmation(visible, { restoreFocus = false } = {}) {
  const confirmation = element("clear-confirmation");
  const clearButton = element("clear-button");

  confirmation.hidden = !visible;
  clearButton.setAttribute("aria-expanded", String(visible));
  element("clear-error").hidden = true;
  element("clear-error").textContent = "";

  if (visible) {
    window.requestAnimationFrame(() => element("clear-cancel").focus());
  } else if (restoreFocus && !clearButton.disabled) {
    clearButton.focus();
  }
}

async function clearActivity() {
  clearing = true;
  element("clear-button").disabled = true;
  element("clear-cancel").disabled = true;
  element("clear-confirm").disabled = true;
  element("clear-error").hidden = true;

  try {
    await invoke("clear_activity");
    entries = [];
    setClearConfirmation(false);
    render();
    await refresh();
  } catch (error) {
    console.error(error);
    element("clear-error").textContent = `${copy.loadFailedTitle}. ${copy.loadFailedDescription}`;
    element("clear-error").hidden = false;
  } finally {
    clearing = false;
    element("clear-cancel").disabled = false;
    element("clear-confirm").disabled = false;
    element("clear-button").disabled = entries.length === 0;
  }
}

function handleFilterKeys(event) {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
    return;
  }

  const buttons = [...document.querySelectorAll("[data-filter]")];
  const current = buttons.indexOf(event.target);
  if (current === -1) {
    return;
  }

  event.preventDefault();
  let next = current;
  if (event.key === "ArrowLeft") {
    next = (current - 1 + buttons.length) % buttons.length;
  } else if (event.key === "ArrowRight") {
    next = (current + 1) % buttons.length;
  } else if (event.key === "Home") {
    next = 0;
  } else if (event.key === "End") {
    next = buttons.length - 1;
  }

  selectFilter(buttons[next].dataset.filter, { focus: true });
}

function wireInteractions() {
  element("activity-search").addEventListener("input", render);
  element("activity-search").addEventListener("keydown", (event) => {
    if (event.key === "Escape" && event.currentTarget.value) {
      event.preventDefault();
      event.currentTarget.value = "";
      render();
    }
  });

  element("activity-filters").addEventListener("click", (event) => {
    const button = event.target.closest("[data-filter]");
    if (button) {
      selectFilter(button.dataset.filter);
    }
  });
  element("activity-filters").addEventListener("keydown", handleFilterKeys);

  element("retry-button").addEventListener("click", () => {
    void refresh({ showLoading: true });
  });
  element("clear-button").addEventListener("click", () => setClearConfirmation(true));
  element("clear-cancel").addEventListener("click", () =>
    setClearConfirmation(false, { restoreFocus: true }),
  );
  element("clear-confirm").addEventListener("click", () => void clearActivity());

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !element("clear-confirmation").hidden) {
      event.preventDefault();
      setClearConfirmation(false, { restoreFocus: true });
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key.toLocaleLowerCase() === "f") {
      event.preventDefault();
      element("activity-search").focus();
      element("activity-search").select();
    }
  });
}

window.addEventListener("beforeunload", () => {
  if (scheduledRefresh !== undefined) {
    window.clearTimeout(scheduledRefresh);
  }
  if (stopListening) {
    stopListening();
  }
});

window.addEventListener("DOMContentLoaded", async () => {
  try {
    copy = await invoke("activity_copy");
  } catch (error) {
    console.error(error);
    document.body.classList.add("ready");
    return;
  }

  applyCopy(copy);
  wireInteractions();
  showContent("loading");
  document.body.classList.add("ready");

  try {
    stopListening = await listen("activity-updated", scheduleRefresh);
  } catch (error) {
    console.error(error);
  }

  await refresh({ showLoading: true });
});
