const { core, event, dialog, store } = window.__TAURI__;

const idleState = document.getElementById('idle-state');
const jobState = document.getElementById('job-state');
const jobNameEl = document.getElementById('job-name');
const folderNameEl = document.getElementById('folder-name');
const overallProgressText = document.getElementById('overall-progress-text');
const overallProgressBar = document.getElementById('overall-progress-bar');
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

    fileProgressByName.set(file.fileName, { element: progress, totalBytes: file.sizeBytes, downloadedBytes: 0 });
  }
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

event.listen('download-progress', (e) => {
  const { fileName, bytesDownloaded, totalBytes, error } = e.payload;
  const entry = fileProgressByName.get(fileName);
  if (!entry) {
    return;
  }

  entry.downloadedBytes = bytesDownloaded;
  entry.totalBytes = totalBytes;
  entry.element.max = totalBytes || 1;
  entry.element.value = bytesDownloaded;

  if (error) {
    showError(`${fileName}: ${error}`);
  }

  updateOverallProgress();
});

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
}

registerDeepLinkHandler();
