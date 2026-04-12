const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const island = document.getElementById('island');
const keys = document.querySelectorAll('.key');
const toolNameEl = document.getElementById('tool-name');
const appWindow = getCurrentWindow();

let selectedIndex = 1; // Default: Allow Always (middle)
let currentRequest = null;

// ---- Mario-style Sound Effects (Web Audio API) ----
const audioCtx = new (window.AudioContext || window.webkitAudioContext)();

function playNote(freq, duration, volume, type, startTime) {
  const osc = audioCtx.createOscillator();
  const gain = audioCtx.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(freq, startTime);
  gain.gain.setValueAtTime(volume, startTime);
  gain.gain.exponentialRampToValueAtTime(0.001, startTime + duration);
  osc.connect(gain).connect(audioCtx.destination);
  osc.start(startTime);
  osc.stop(startTime + duration);
}

function playSound(action) {
  const now = audioCtx.currentTime;

  if (action === 'allow-once') {
    // Single coin: B5 -> E6
    playNote(988, 0.08, 0.16, 'square', now);
    playNote(1319, 0.35, 0.16, 'square', now + 0.08);
  } else if (action === 'allow-always') {
    // Multi coins: C6 -> E6 -> G6 -> C7 rapid, then E7 finale
    const notes = [1047, 1319, 1568, 2093];
    notes.forEach((freq, i) => {
      playNote(freq, 0.08, 0.15, 'square', now + i * 0.1);
    });
    playNote(2637, 0.3, 0.16, 'square', now + 0.4);
  } else if (action === 'deny') {
    // Failure: descending half-steps, triangle wave
    const deathNotes = [494, 466, 440, 415, 392, 370];
    deathNotes.forEach((freq, i) => {
      playNote(freq, 0.1, 0.14 - i * 0.01, 'triangle', now + i * 0.1);
    });
    playNote(330, 0.2, 0.08, 'triangle', now + 0.6);
  }
}

// Window sizes (logical pixels)
const COMPACT_SIZE = { width: 220, height: 52 };
const EXPANDED_SIZE = { width: 480, height: 150 };

// Update visual selection
function updateSelection() {
  keys.forEach((key, i) => {
    key.classList.toggle('selected', i === selectedIndex);
  });
}

// Resize window and re-center horizontally
async function resizeWindow(size) {
  try {
    const monitor = await appWindow.currentMonitor();
    if (monitor) {
      const scale = monitor.scaleFactor;
      const screenW = monitor.size.width / scale;
      const x = (screenW - size.width) / 2;
      const y = 38;
      await appWindow.setSize(new window.__TAURI__.window.LogicalSize(size.width, size.height));
      await appWindow.setPosition(new window.__TAURI__.window.LogicalPosition(x, y));
    } else {
      await appWindow.setSize(new window.__TAURI__.window.LogicalSize(size.width, size.height));
    }
  } catch (e) {
    console.error('Failed to resize window:', e);
  }
}

// Show permission UI — expand the island
async function showPermission(event) {
  currentRequest = event;
  const toolName = event.tool || 'Unknown Tool';
  toolNameEl.textContent = toolName;

  selectedIndex = 1; // Reset to Allow Always
  updateSelection();

  // Resize window first, then animate island
  await resizeWindow(EXPANDED_SIZE);
  requestAnimationFrame(() => {
    island.classList.remove('compact');
    island.classList.add('expanded');
  });
}

// Hide permission UI — shrink back to pill
async function hidePermission() {
  island.classList.remove('expanded');
  island.classList.add('compact');
  currentRequest = null;

  // Wait for CSS animation to finish, then shrink window
  setTimeout(() => {
    resizeWindow(COMPACT_SIZE);
  }, 350);
}

// Confirm selection
async function confirmSelection() {
  if (!currentRequest) return;

  const key = keys[selectedIndex];
  const action = key.dataset.action;

  // Press animation
  key.classList.add('pressed');
  setTimeout(() => key.classList.remove('pressed'), 100);

  let decision;
  switch (action) {
    case 'deny':
      decision = 'deny';
      break;
    case 'allow-always':
      decision = 'allow-always';
      break;
    case 'allow-once':
      decision = 'allow';
      break;
  }

  try {
    await invoke('respond_permission', {
      decision: decision,
      toolName: currentRequest.tool || '',
    });
  } catch (e) {
    console.error('Failed to respond:', e);
  }

  // Play Mario sound effect
  playSound(action);

  hidePermission();
}

// Keyboard navigation
document.addEventListener('keydown', (e) => {
  if (!currentRequest) return;

  switch (e.key) {
    case 'ArrowLeft':
      e.preventDefault();
      selectedIndex = Math.max(0, selectedIndex - 1);
      updateSelection();
      break;
    case 'ArrowRight':
      e.preventDefault();
      selectedIndex = Math.min(keys.length - 1, selectedIndex + 1);
      updateSelection();
      break;
    case 'Enter':
      e.preventDefault();
      confirmSelection();
      break;
    case 'Escape':
      e.preventDefault();
      selectedIndex = 2; // Reject is now on the right (index 2)
      updateSelection();
      confirmSelection();
      break;
  }
});

// Mouse click support
keys.forEach((key, i) => {
  key.addEventListener('click', () => {
    if (!currentRequest) return;
    selectedIndex = i;
    updateSelection();
    confirmSelection();
  });
});

// Listen for permission events from Rust backend
listen('permission-request', (event) => {
  showPermission(event.payload);
});

// Listen for auto-approved notifications
listen('permission-auto-approved', (event) => {
  console.log('Auto-approved:', event.payload);
});

// Initialize: set compact size on load
resizeWindow(COMPACT_SIZE);
