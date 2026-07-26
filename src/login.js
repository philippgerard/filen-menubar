const { invoke } = window.__TAURI__.core;

let copy;

const element = (id) => document.getElementById(id);

function applyCopy(values) {
  document.title = values.windowTitle;
  document.documentElement.lang = values.locale;
  document.documentElement.dataset.platform = values.platform;

  const text = {
    title: values.title,
    intro: values.intro,
    "email-label": values.emailLabel,
    "password-label": values.passwordLabel,
    "persist-note": values.persistNote,
    "alternative-title": values.alternativeTitle,
    "alternative-note": values.alternativeNote,
    cancel: values.cancel,
    submit: values.submit,
    "two-factor-title": values.twoFactorTitle,
    "two-factor-intro": values.twoFactorIntro,
    "two-factor-label": values.twoFactorLabel,
    "two-factor-cancel": values.cancel,
    verify: values.verify,
    success: values.success,
  };

  for (const [id, value] of Object.entries(text)) {
    element(id).textContent = value;
  }

  element("email").placeholder = values.emailPlaceholder;
  element("password").placeholder = values.passwordPlaceholder;
  element("two-factor-code").placeholder = values.twoFactorPlaceholder;
}

function errorFor(status) {
  const errors = {
    missingFields: copy.errorMissingFields,
    invalidCredentials: copy.errorInvalidCredentials,
    invalidTwoFactor: copy.errorInvalidTwoFactor,
    keychainUnavailable: copy.errorKeychainUnavailable,
    busy: copy.errorBusy,
    timeout: copy.errorTimeout,
    failed: copy.errorFailed,
    noActiveLogin: copy.errorFailed,
    cancelled: copy.errorFailed,
  };
  return errors[status] ?? copy.errorFailed;
}

function setBusy(form, button, busy, busyLabel, idleLabel) {
  for (const control of form.elements) {
    control.disabled = busy;
  }
  button.textContent = busy ? busyLabel : idleLabel;
  button.classList.toggle("loading", busy);
}

function showTwoFactor() {
  element("credentials-step").hidden = true;
  element("two-factor-step").hidden = false;
  element("two-factor-code").focus();
}

async function finishSuccess() {
  element("credentials-step").hidden = true;
  element("two-factor-step").hidden = true;
  element("success-step").hidden = false;
  window.setTimeout(() => invoke("close_login"), 650);
}

async function submitCredentials(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const submit = element("submit");
  const error = element("credentials-error");
  const emailInput = element("email");
  const passwordInput = element("password");

  error.textContent = "";
  if (!emailInput.value.trim() || !passwordInput.value) {
    error.textContent = copy.errorMissingFields;
    return;
  }

  setBusy(form, submit, true, copy.authenticating, copy.submit);

  try {
    const result = await invoke("start_login", {
      email: emailInput.value.trim(),
      password: passwordInput.value,
    });
    passwordInput.value = "";

    if (result.status === "needsTwoFactor") {
      showTwoFactor();
    } else if (result.status === "success") {
      await finishSuccess();
    } else {
      error.textContent = errorFor(result.status);
    }
  } catch {
    passwordInput.value = "";
    error.textContent = copy.errorFailed;
  } finally {
    setBusy(form, submit, false, copy.authenticating, copy.submit);
  }
}

async function submitTwoFactor(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const verify = element("verify");
  const error = element("two-factor-error");
  const codeInput = element("two-factor-code");

  error.textContent = "";
  if (!codeInput.value.trim()) {
    error.textContent = copy.errorMissingFields;
    return;
  }

  setBusy(form, verify, true, copy.verifying, copy.verify);

  try {
    const result = await invoke("submit_two_factor", {
      twoFactorCode: codeInput.value.trim(),
    });
    codeInput.value = "";

    if (result.status === "success") {
      await finishSuccess();
    } else {
      error.textContent = errorFor(result.status);
    }
  } catch {
    codeInput.value = "";
    error.textContent = copy.errorFailed;
  } finally {
    setBusy(form, verify, false, copy.verifying, copy.verify);
  }
}

async function cancel() {
  await invoke("cancel_login");
}

window.addEventListener("DOMContentLoaded", async () => {
  copy = await invoke("login_copy");
  applyCopy(copy);

  element("credentials-form").addEventListener("submit", submitCredentials);
  element("two-factor-form").addEventListener("submit", submitTwoFactor);
  for (const button of document.querySelectorAll("[data-cancel]")) {
    button.addEventListener("click", cancel);
  }

  element("email").focus();
  document.body.classList.add("ready");
});
