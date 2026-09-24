const { core, event, dialog, store } = window.__TAURI__;

const idleState = document.getElementById('idle-state');
const jobState = document.getElementById('job-state');
const jobNameEl = document.getElementById('job-name');
const folderNameEl = document.getElementById('folder-name');
const overallProgressText = document.getElementById('overall-progress-text');
const overallProgressBar = document.getElementById('overall-progress-bar');
const fileCountText = document.getElementById('file-count-text');
const retryFailedButton = document.getElementById('retry-failed-button');
const fileListEl = document.getElementById('file-list');
const errorMessageEl = document.getElementById('error-message');
const queueStatusEl = document.getElementById('queue-status');
const queueStatusTextEl = document.getElementById('queue-status-text');
const queueListEl = document.getElementById('queue-list');
const editorLoginLink = document.getElementById('editor-login-link');
const editorLoginForm = document.getElementById('editor-login-form');
const editorMobileInput = document.getElementById('editor-mobile-input');
const editorPasswordInput = document.getElementById('editor-password-input');
const editorLoginCancel = document.getElementById('editor-login-cancel');
const editorLoginError = document.getElementById('editor-login-error');
const editorLoggedInState = document.getElementById('editor-logged-in-state');
const editorNameEl = document.getElementById('editor-name');
const editorRoleEl = document.getElementById('editor-role');
const editorLogoutLink = document.getElementById('editor-logout-link');

let currentEditorSession = null; // { photographerId, name, role, accessToken } or null when logged out

const DESTINATION_STORE_KEY = 'destinationDir';
let destinationStore = null;
let currentManifest = null;
let fileProgressByName = new Map();

// Only one job's downloads run at a time. A "Download with App" click that arrives while
// isDownloading is true gets queued instead of overwriting the currently-shown job's manifest
// and progress tracking (which used to silently orphan the in-progress job -- its downloads kept
// running in the background, but the UI dropped all its progress events since fileProgressByName
// had already been rebuilt for the new job).
let isDownloading = false;
let jobQueue = [];

async function getDestinationStore() {
  if (!destinationStore) {
    destinationStore = await store.load('settings.json');
  }
  return destinationStore;
}

async function resolveDestinationDir() {
  const settings = await getDestinationStore();
  const remembered = await settings.get(DESTINATION_STORE_KEY);
  if (remembered) {
    return remembered;
  }

  const chosen = await dialog.open({ directory: true, title: 'Choose a folder to save downloads to' });
  if (!chosen) {
    throw new Error('A destination folder is required to start the download.');
  }

  await settings.set(DESTINATION_STORE_KEY, chosen);
  await settings.save();
  return chosen;
}

function showError(message) {
  errorMessageEl.textContent = message;
  errorMessageEl.hidden = false;
}

function renderFileList(files) {
  fileListEl.replaceChildren();
  fileProgressByName.clear();

  for (const file of files) {
    const li = document.createElement('li');
    const label = document.createElement('span');
    label.textContent = file.fileName;

    const progress = document.createElement('progress');
    progress.max = 100;
    progress.value = 0;

    li.appendChild(label);
    li.appendChild(progress);
    fileListEl.appendChild(li);

    fileProgressByName.set(file.fileName, {
      element: progress,
      totalBytes: file.sizeBytes,
      downloadedBytes: 0,
      done: false,
      failed: false
    });
  }

  updateFileCountAndRetryUi();
}

function updateOverallProgress() {
  let totalBytes = 0;
  let downloadedBytes = 0;
  for (const entry of fileProgressByName.values()) {
    totalBytes += entry.totalBytes;
    downloadedBytes += entry.downloadedBytes;
  }

  const percent = totalBytes > 0 ? Math.round((downloadedBytes / totalBytes) * 100) : 0;
  overallProgressBar.value = percent;
  overallProgressText.textContent = `${percent}%`;
}

function updateFileCountAndRetryUi() {
  const total = fileProgressByName.size;
  let succeeded = 0;
  let failed = 0;
  for (const entry of fileProgressByName.values()) {
    if (entry.done && !entry.failed) {
      succeeded++;
    } else if (entry.failed) {
      failed++;
    }
  }

  fileCountText.textContent = failed > 0
    ? `${succeeded} of ${total} files (${failed} failed)`
    : `${succeeded} of ${total} files`;

  retryFailedButton.hidden = failed === 0;
}

function hasAnyFailures() {
  for (const entry of fileProgressByName.values()) {
    if (entry.failed) {
      return true;
    }
  }
  return false;
}

function describeQueuedJob(queuedJob) {
  return queuedJob.manifest.folderName
    ? `${queuedJob.manifest.jobName} (${queuedJob.manifest.folderName})`
    : queuedJob.manifest.jobName;
}

function updateQueueStatus() {
  if (jobQueue.length === 0) {
    queueStatusEl.hidden = true;
    return;
  }

  queueStatusEl.hidden = false;
  queueStatusTextEl.textContent = hasAnyFailures()
    ? 'Waiting to continue (retry the failed files above) -- next up:'
    : 'Queued, in order:';

  queueListEl.replaceChildren();
  for (const queuedJob of jobQueue) {
    const li = document.createElement('li');
    li.textContent = describeQueuedJob(queuedJob);
    queueListEl.appendChild(li);
  }
}

event.listen('download-progress', (e) => {
  const { fileName, bytesDownloaded, totalBytes, done, error } = e.payload;
  const entry = fileProgressByName.get(fileName);
  if (!entry) {
    return;
  }

  entry.downloadedBytes = bytesDownloaded;
  entry.totalBytes = totalBytes;
  entry.done = done;
  entry.failed = Boolean(error);
  entry.element.max = totalBytes || 1;
  entry.element.value = bytesDownloaded;

  if (error) {
    showError(`${fileName}: ${error}`);
  }

  updateOverallProgress();
  updateFileCountAndRetryUi();
});

async function retryFailedDownloads() {
  if (!currentManifest) {
    return;
  }

  const failedFiles = currentManifest.files.filter((file) => {
    const entry = fileProgressByName.get(file.fileName);
    return entry && entry.failed;
  });

  if (failedFiles.length === 0) {
    return;
  }

  errorMessageEl.hidden = true;
  retryFailedButton.hidden = true;

  // Clear each retried file's failed/done state up front so the counts and progress bars reflect
  // "in progress again" immediately, rather than still showing the old failure until the first
  // new progress event arrives for that file.
  for (const file of failedFiles) {
    const entry = fileProgressByName.get(file.fileName);
    if (entry) {
      entry.failed = false;
      entry.done = false;
    }
  }
  updateFileCountAndRetryUi();

  let destinationDir;
  try {
    destinationDir = await resolveDestinationDir();
  } catch (e) {
    showError(String(e));
    return;
  }

  // A manifest containing only the failed files, scoped to the same job/folder so it lands in
  // the same destination directory the original download used -- already-complete files are
  // simply left alone since they're not part of this retry manifest at all.
  const retryManifest = {
    jobName: currentManifest.jobName,
    folderName: currentManifest.folderName,
    files: failedFiles
  };

  try {
    await core.invoke('download_all_command', {
      manifest: retryManifest,
      destinationDir,
      accessToken: currentEditorSession?.accessToken ?? null
    });
    // download_all_command only resolves (rather than rejecting) once every file in this retry
    // batch has actually succeeded -- treat that as the authoritative outcome and finalize each
    // retried file's state explicitly, rather than relying solely on this batch's individual
    // download-progress events having each been received and processed. This is the fix for a
    // real bug found in testing: the on-disk files were correctly downloaded, but the "N of M
    // files" count never advanced for them, suggesting their final progress events weren't
    // reliably reflected in fileProgressByName by the time the invoke settled.
    for (const file of failedFiles) {
      const entry = fileProgressByName.get(file.fileName);
      if (entry) {
        entry.done = true;
        entry.failed = false;
        entry.downloadedBytes = entry.totalBytes;
        entry.element.value = entry.totalBytes;
      }
    }
  } catch (e) {
    showError(`Retry finished with an error: ${e}`);
  }

  updateOverallProgress();
  updateFileCountAndRetryUi();

  // A successful retry may have just cleared the last failure blocking the queue -- check.
  await advanceQueueIfReady();
}

retryFailedButton.addEventListener('click', retryFailedDownloads);

async function runDownloadJob(manifestUrl, prefetchedManifest) {
  idleState.hidden = true;
  jobState.hidden = false;
  errorMessageEl.hidden = true;

  if (prefetchedManifest) {
    // Already fetched when this job was queued (see startDownload), so its name could be shown
    // in the queue status right away and its file-level download tokens are already resolved --
    // no need to fetch the manifest link a second time.
    currentManifest = prefetchedManifest;
  } else {
    try {
      currentManifest = await core.invoke('fetch_manifest_command', {
        manifestUrl,
        accessToken: currentEditorSession?.accessToken ?? null
      });
    } catch (e) {
      showError(`Could not load the file list: ${e}`);
      return;
    }
  }

  jobNameEl.textContent = currentManifest.jobName;
  folderNameEl.textContent = currentManifest.folderName ? `Folder: ${currentManifest.folderName}` : '';
  renderFileList(currentManifest.files);

  let destinationDir;
  try {
    destinationDir = await resolveDestinationDir();
  } catch (e) {
    showError(String(e));
    return;
  }

  try {
    await core.invoke('download_all_command', {
      manifest: currentManifest,
      destinationDir,
      accessToken: currentEditorSession?.accessToken ?? null
    });
    // Same authoritative-finalization backstop as retryFailedDownloads: a successful invoke
    // means every file in the manifest succeeded, so finalize them all explicitly rather than
    // depending solely on each file's own download-progress events having landed.
    for (const entry of fileProgressByName.values()) {
      entry.done = true;
      entry.failed = false;
      entry.downloadedBytes = entry.totalBytes;
      entry.element.value = entry.totalBytes;
    }
  } catch (e) {
    showError(`Download finished with an error: ${e}`);
  }

  updateOverallProgress();
  updateFileCountAndRetryUi();
}

// Called once the currently-displayed job has settled (finished, errored, or just been retried).
// Moves on to the next queued job only if nothing about the current job still needs attention --
// if it has any failed files, the queue stays paused so a "Retry Failed Downloads" click doesn't
// get raced by the next job silently taking over the UI first.
async function advanceQueueIfReady() {
  if (hasAnyFailures()) {
    updateQueueStatus();
    return;
  }

  if (jobQueue.length === 0) {
    isDownloading = false;
    updateQueueStatus();
    return;
  }

  const next = jobQueue.shift();
  updateQueueStatus();
  await runDownloadJob(next.manifestUrl, next.manifest);
  await advanceQueueIfReady();
}

async function startDownload(manifestUrl) {
  if (isDownloading) {
    // Fetch the manifest now (not when this job's turn eventually comes) so its name can be
    // shown in the queue status right away, and so its file-level tokens are resolved up front
    // rather than only once the job actually starts downloading.
    let manifest;
    try {
      manifest = await core.invoke('fetch_manifest_command', {
      manifestUrl,
      accessToken: currentEditorSession?.accessToken ?? null
    });
    } catch (e) {
      showError(`Could not load the file list for a queued job: ${e}`);
      return;
    }
    jobQueue.push({ manifestUrl, manifest });
    updateQueueStatus();
    return;
  }

  isDownloading = true;
  await runDownloadJob(manifestUrl);
  await advanceQueueIfReady();
}

function extractManifestUrlFromDeepLink(url) {
  // url looks like: vsnapu-download://fetch?manifest=<url-encoded manifest link>
  const parsed = new URL(url);
  const manifestParam = parsed.searchParams.get('manifest');
  return manifestParam ? decodeURIComponent(manifestParam) : null;
}

async function registerDeepLinkHandler() {
  const { deepLink } = window.__TAURI__;
  await deepLink.onOpenUrl((urls) => {
    for (const url of urls) {
      const manifestUrl = extractManifestUrlFromDeepLink(url);
      if (manifestUrl) {
        startDownload(manifestUrl);
        return;
      }
    }
  });

  // Handles the cold-start case: this process was launched directly with a vsnapu-download://
  // URL as its argument (the app wasn't already running). Emitted once, at most, by the Rust
  // side's .setup() hook via tauri-plugin-deep-link's get_current() -- NOT by the
  // single-instance plugin's callback (that callback is intentionally empty; see lib.rs), so
  // this cannot double-fire the way it did before that fix.
  await event.listen('deep-link-url', (e) => {
    const manifestUrl = extractManifestUrlFromDeepLink(e.payload);
    if (manifestUrl) {
      startDownload(manifestUrl);
    }
  });
}

function showLoggedOutState() {
  currentEditorSession = null;
  editorLoginLink.hidden = false;
  editorLoginForm.hidden = true;
  editorLoggedInState.hidden = true;
}

function showLoggedInState(session) {
  currentEditorSession = session;
  editorLoginLink.hidden = true;
  editorLoginForm.hidden = true;
  editorLoggedInState.hidden = false;
  editorNameEl.textContent = session.name || '';
  editorRoleEl.textContent = session.role || '';
}

editorLoginLink.addEventListener('click', (e) => {
  e.preventDefault();
  editorLoginError.hidden = true;
  editorLoginLink.hidden = true;
  editorLoginForm.hidden = false;
});

editorLoginCancel.addEventListener('click', () => {
  editorLoginForm.hidden = true;
  editorLoginLink.hidden = false;
});

editorLoginForm.addEventListener('submit', async (e) => {
  e.preventDefault();
  editorLoginError.hidden = true;

  try {
    const session = await core.invoke('editor_login_command', {
      mobile: editorMobileInput.value,
      password: editorPasswordInput.value
    });
    editorPasswordInput.value = '';
    showLoggedInState(session);
  } catch (err) {
    editorLoginError.textContent = String(err);
    editorLoginError.hidden = false;
  }
});

editorLogoutLink.addEventListener('click', async (e) => {
  e.preventDefault();
  try {
    await core.invoke('editor_logout_command');
  } catch (err) {
    // Best-effort server-side clear (see editor_session::logout) -- local state is cleared
    // either way, so a network error here is not shown to the user.
  }
  showLoggedOutState();
});

async function restoreEditorSessionIfAny() {
  try {
    const session = await core.invoke('editor_refresh_command');
    if (session) {
      // Name/role aren't returned by a silent refresh (see editor_session::refresh) -- keep
      // whatever was already displayed if this is a re-refresh, otherwise show a generic label
      // until the next full login.
      showLoggedInState({
        name: currentEditorSession?.name || session.photographerId,
        role: currentEditorSession?.role || '',
        photographerId: session.photographerId,
        accessToken: session.accessToken
      });
    } else {
      showLoggedOutState();
    }
  } catch (err) {
    showLoggedOutState();
  }
}

restoreEditorSessionIfAny();

registerDeepLinkHandler();
