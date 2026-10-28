const DATABASE = "hyz-study-ink";
const STORE = "drafts";
const memoryDrafts = new Map();
const pendingWrites = new Map();
let databasePromise;

function openDatabase() {
  if (!databasePromise) {
    databasePromise = new Promise((resolve, reject) => {
      const request = indexedDB.open(DATABASE, 1);
      request.onupgradeneeded = () => {
        request.result.createObjectStore(STORE, { keyPath: "scope" });
      };
      request.onerror = () => reject(request.error);
      request.onblocked = () => reject(new Error("Draft database upgrade blocked"));
      request.onsuccess = () => {
        const db = request.result;
        db.onversionchange = () => {
          db.close();
          databasePromise = undefined;
        };
        resolve(db);
      };
    }).catch(error => {
      databasePromise = undefined;
      throw error;
    });
  }
  return databasePromise;
}

async function transaction(mode, operation) {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const request = operation(tx.objectStore(STORE));
    tx.oncomplete = () => resolve(request.result);
    tx.onabort = () => reject(tx.error || new Error("Draft transaction aborted"));
    tx.onerror = () => reject(tx.error || new Error("Draft transaction failed"));
  });
}

function validateScope(scope) {
  if (!scope || scope.length > 120) throw new Error("Invalid draft scope");
}

function snapshot(draft) {
  return JSON.stringify({ history: JSON.parse(draft.data), persisted: draft.persisted });
}

export async function saveInk(scope, data) {
  validateScope(scope);
  const draft = { data, persisted: false };
  // Navigation can restore this snapshot even if the disk write fails.
  memoryDrafts.set(scope, draft);
  const previous = pendingWrites.get(scope) || Promise.resolve();
  const write = previous.catch(() => {}).then(() => transaction("readwrite", store => store.put({
    scope, version: 1, history: data, updatedAt: Date.now(),
  })));
  pendingWrites.set(scope, write);
  try {
    await write;
    draft.persisted = true;
  } finally {
    if (pendingWrites.get(scope) === write) pendingWrites.delete(scope);
  }
}

async function cachedDraft(scope) {
  while (pendingWrites.has(scope)) {
    try {
      await pendingWrites.get(scope);
    } catch (_) {
      // A failed write still leaves its complete history in memory.
    }
  }
  return snapshot(memoryDrafts.get(scope));
}

export async function loadInk(scope) {
  validateScope(scope);
  if (memoryDrafts.has(scope)) return cachedDraft(scope);
  let record;
  let readError;
  try {
    record = await transaction("readonly", store => store.get(scope));
  } catch (error) {
    readError = error;
  }
  // A stroke written during the asynchronous read always takes precedence.
  if (memoryDrafts.has(scope)) return cachedDraft(scope);
  if (record) {
    if (record.version !== 1 || typeof record.history !== "string") {
      throw new Error("Unsupported draft format");
    }
    const draft = { data: record.history, persisted: true };
    const result = snapshot(draft);
    memoryDrafts.set(scope, draft);
    return result;
  }
  if (readError) throw readError;
  const draft = { data: "[]", persisted: true };
  memoryDrafts.set(scope, draft);
  return snapshot(draft);
}

export async function flushInk() {
  await Promise.all(pendingWrites.values());
  await Promise.all([...memoryDrafts.entries()]
    .filter(([, draft]) => !draft.persisted)
    .map(([scope, draft]) => saveInk(scope, draft.data)));
}
