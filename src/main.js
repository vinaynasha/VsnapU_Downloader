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

const DESTINATION_STORE_KEY = 'destinationDir';
let destinationStore = null;
let currentManifest = null;
let fileProgressByName = new Map();

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
    await core.invoke('download_all_command', { manifest: retryManifest, destinationDir });
  } catch (e) {
    showError(`Retry finished with an error: ${e}`);
  }
}

retryFailedButton.addEventListener('click', retryFailedDownloads);

async function startDownload(manifestUrl) {
  idleState.hidden = true;
  jobState.hidden = false;
  errorMessageEl.hidden = true;

  try {
    currentManifest = await core.invoke('fetch_manifest_command', { manifestUrl });
  } catch (e) {
    showError(`Could not load the file list: ${e}`);
    return;
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
    await core.invoke('download_all_command', { manifest: currentManifest, destinationDir });
  } catch (e) {
    showError(`Download finished with an error: ${e}`);
  }
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

registerDeepLinkHandler();
