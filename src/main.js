const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const island = document.getElementById('island');
const keys = document.querySelectorAll('.key');
const toolNameEl = document.getElementById('tool-name');
const toolDescEl = document.getElementById('tool-desc');
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
const EXPANDED_SIZE = { width: 520, height: 188 };

// Update visual selection
function updateSelection() {
  keys.forEach((key, i) => {
    key.classList.toggle('selected', i === selectedIndex);
  });
}

// Format tool_input into a brief human-readable description
function formatToolInput(toolInput) {
  if (!toolInput) return '';
  try {
    if (typeof toolInput.command === 'string') return toolInput.command;
    if (typeof toolInput.file_path === 'string') return toolInput.file_path;
    if (typeof toolInput.path === 'string') return toolInput.path;
    if (typeof toolInput.new_path === 'string') return toolInput.new_path;
    if (typeof toolInput.url === 'string') return toolInput.url;
    if (typeof toolInput.content === 'string') {
      const s = toolInput.content.trim();
      return s.length > 80 ? s.substring(0, 80) + '…' : s;
    }
    // Fallback: first string value
    const vals = Object.values(toolInput).filter(v => typeof v === 'string');
    if (vals.length > 0) {
      const s = vals[0].trim();
      return s.length > 80 ? s.substring(0, 80) + '…' : s;
    }
    return '';
  } catch (e) {
    return '';
  }
}

// Resize window and re-center horizontally on primary monitor
async function resizeWindow(size) {
  try {
    // Prefer primary monitor for consistent top-center placement
    const monitor = await appWindow.primaryMonitor() || await appWindow.currentMonitor();
    if (monitor) {
      const scale = monitor.scaleFactor;
      const screenW = monitor.size.width / scale;
      // Include monitor's own x-offset (for multi-monitor setups)
      const monitorX = monitor.position ? monitor.position.x / scale : 0;
      const x = monitorX + (screenW - size.width) / 2;
      const isMac = navigator.platform.startsWith('Mac') || navigator.userAgent.includes('Mac');
      const y = isMac ? 38 : 8;
      await appWindow.setSize(new window.__TAURI__.window.LogicalSize(size.width, size.height));
      await appWindow.setPosition(new window.__TAURI__.window.LogicalPosition(x, y));
    } else {
      await appWindow.setSize(new window.__TAURI__.window.LogicalSize(size.width, size.height));
    }
  } catch (e) {
    console.error('Failed to resize window:', e);
  }
}

// Show permission UI — resize (hidden), then show + focus + expand
async function showPermission(event) {
  currentRequest = event;
  const toolName = event.tool || 'Unknown Tool';
  toolNameEl.textContent = toolName;

  // Populate tool description
  if (toolDescEl) {
    toolDescEl.textContent = formatToolInput(event.tool_input);
  }

  selectedIndex = 1; // Reset to Allow Always
  updateSelection();

  // Resize and position while still hidden, then reveal
  await resizeWindow(EXPANDED_SIZE);

  try {
    await appWindow.show();
    await appWindow.setFocus();
  } catch (e) {
    console.error('Failed to show/focus window:', e);
  }

  requestAnimationFrame(() => {
    island.classList.remove('compact');
    island.classList.add('expanded');
  });
}

// Hide permission UI — collapse island, then hide window
async function hidePermission() {
  island.classList.remove('expanded');
  island.classList.add('compact');
  currentRequest = null;

  // Reset mic indicator
  if (micStatus) {
    micStatus.className = 'mic-status mic-idle';
    const mt = micStatus.querySelector('.mic-text');
    if (mt) mt.textContent = '';
  }

  // Clear tool description
  if (toolDescEl) toolDescEl.textContent = '';

  // Wait for CSS animation to finish, then shrink and hide window
  setTimeout(async () => {
    await resizeWindow(COMPACT_SIZE);
    try {
      await appWindow.hide();
    } catch (e) {
      console.error('Failed to hide window:', e);
    }
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
      selectedIndex = 2; // Reject is on the right (index 2)
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

// ---- Voice Status ----
const micStatus = document.getElementById('mic-status');
const micText = micStatus?.querySelector('.mic-text');

const voiceStatusMap = {
  idle: { class: 'mic-idle', text: '待机' },
  listening: { class: 'mic-listening', text: '监听中' },
  processing: { class: 'mic-processing', text: '识别中' },
  recognized: { class: 'mic-recognized', text: '' },
};

function handleVoiceStatus(event) {
  const { status, text, command } = event.payload;
  const config = voiceStatusMap[status] || voiceStatusMap.idle;

  if (micStatus) {
    micStatus.className = 'mic-status ' + config.class;

    if (micText) {
      if (status === 'recognized' && command) {
        const commandLabels = {
          'allow': 'Once ✓',
          'allow-always': 'Always ✓',
          'deny': 'Reject ✗'
        };
        micText.textContent = commandLabels[command] || text || config.text;
      } else {
        micText.textContent = config.text;
      }
    }
  }
}

// Listen for voice command (triggers confirmation)
function handleVoiceCommand(event) {
  if (!currentRequest) return;
  const { decision } = event.payload;

  const actionMap = {
    'allow': 'allow-once',
    'allow-once': 'allow-once',
    'allow-always': 'allow-always',
    'deny': 'deny',
  };
  const action = actionMap[decision];
  if (!action) return;

  const targetKey = document.querySelector(`.key[data-action="${action}"]`);
  if (!targetKey) return;

  const idx = Array.from(keys).indexOf(targetKey);
  if (idx === -1) return;

  selectedIndex = idx;
  updateSelection();

  setTimeout(() => {
    confirmSelection();
  }, 150);
}

async function init() {
  await Promise.all([
    listen('permission-request', (event) => {
      showPermission(event.payload);
    }),
    listen('permission-auto-approved', (event) => {
      console.log('Auto-approved:', event.payload);
    }),
    listen('voice-status', handleVoiceStatus),
    listen('voice-command', handleVoiceCommand),
  ]);

  try {
    const pending = await invoke('get_pending_permission');
    if (pending) {
      await showPermission(pending);
      return;
    }
  } catch (e) {
    console.error('Failed to load pending permission:', e);
  }

  resizeWindow(COMPACT_SIZE);
}

init();
