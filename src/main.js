const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const keys = document.querySelectorAll('.key');
const app = document.getElementById('app');
const idle = document.getElementById('idle');
const toolNameEl = document.getElementById('tool-name');

let selectedIndex = 1; // Default: Allow Always (middle)
let currentRequest = null;

// Update visual selection
function updateSelection() {
  keys.forEach((key, i) => {
    key.classList.toggle('selected', i === selectedIndex);
  });
}

// Show permission UI
function showPermission(event) {
  currentRequest = event;
  const toolName = event.tool || 'Unknown Tool';
  toolNameEl.textContent = toolName;
  idle.style.display = 'none';
  app.classList.remove('hidden');
  selectedIndex = 1; // Reset to Allow Always
  updateSelection();
}

// Hide permission UI
function hidePermission() {
  app.classList.add('hidden');
  idle.style.display = 'flex';
  currentRequest = null;
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
      // Escape = deny
      selectedIndex = 0;
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

// Listen for auto-approved notifications (for visual feedback)
listen('permission-auto-approved', (event) => {
  // Could show a brief toast, for now just log
  console.log('Auto-approved:', event.payload);
});
