const views = {
  overview: {
    file: "overview.png",
    caption: "Overview / See the whole desk.",
    alt: "Alnair Overview dashboard showing usage totals and live traffic with sample data",
  },
  connections: {
    file: "connections.png",
    caption: "Connections / Bring your providers together.",
    alt: "Alnair Connections dashboard showing configured upstream connections and masked keys with sample data",
  },
  usage: {
    file: "usage.png",
    caption: "Usage / Follow the tokens.",
    alt: "Alnair Usage dashboard with a token trend chart and request filters using sample data",
  },
};

const image = document.querySelector("#gallery-image");
const caption = document.querySelector("#gallery-caption");
const galleryStatus = document.querySelector("#gallery-status");
const dialog = document.querySelector("#screenshot-dialog");
const dialogImage = document.querySelector("#dialog-image");
document.querySelector(".gallery-controls").hidden = false;
document.querySelector("#expand-image").hidden = false;
document.querySelector(".install-toolbar").hidden = false;
image.addEventListener("load", () => {
  galleryStatus.textContent = "";
});
image.addEventListener("error", () => {
  galleryStatus.textContent =
    "Screenshot unavailable. Choose another view or use the dashboard screenshots link below.";
});

document.querySelectorAll("[data-view]").forEach((button) => {
  button.addEventListener("click", () => {
    const view = views[button.dataset.view];
    galleryStatus.textContent = "Loading screenshot…";
    image.src = `/assets/${view.file}`;
    image.alt = view.alt;
    caption.textContent = view.caption;
    document.querySelectorAll("[data-view]").forEach((item) => {
      item.setAttribute("aria-pressed", String(item === button));
    });
  });
});

document.querySelector("#expand-image").addEventListener("click", () => {
  dialogImage.src = image.src;
  dialogImage.alt = image.alt;
  document.querySelector("#dialog-title").textContent = caption.textContent;
  dialog.showModal();
});
document
  .querySelector("#close-dialog")
  .addEventListener("click", () => dialog.close());
dialog.addEventListener("click", (event) => {
  if (event.target === dialog) {
    const bounds = dialog.getBoundingClientRect();
    if (
      event.clientX < bounds.left ||
      event.clientX > bounds.right ||
      event.clientY < bounds.top ||
      event.clientY > bounds.bottom
    )
      dialog.close();
  }
});

const methods = {
  npm: {
    language: "Shell",
    command: "npm install -g @xflawlessdev/alnair-router",
    help: "Run alnair-router start to launch the router. To also enable auto-start with your session, run alnair-router install.",
  },
  unix: {
    language: "Shell",
    command:
      "curl -fsSL https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.sh | sh",
    help: "For Linux x86_64/arm64 and Apple Silicon. The script verifies the release checksum, installs the binary, and configures auto-start.",
  },
  windows: {
    language: "PowerShell",
    command:
      "irm https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.ps1 | iex",
    help: "For Windows x64. The script verifies the release checksum, adds the binary to your user PATH, and configures auto-start.",
  },
};
const select = document.querySelector("#install-method");
const command = document.querySelector("#install-command");
const status = document.querySelector("#copy-status");
const copy = document.querySelector("#copy-command");
copy.hidden = false;
let copying = false;

select.addEventListener("change", () => {
  const method = methods[select.value];
  command.textContent = method.command;
  document.querySelector("#command-language").textContent = method.language;
  document.querySelector("#install-help").textContent = method.help;
  status.textContent = "";
});

copy.addEventListener("click", async () => {
  if (copying) return;
  copying = true;
  copy.disabled = true;
  const commandText = command.textContent;
  status.textContent = "Copying command…";
  try {
    await navigator.clipboard.writeText(commandText);
    status.textContent = "Command copied.";
  } catch {
    status.textContent =
      "Clipboard unavailable. Select the command above to copy it manually.";
    const range = document.createRange();
    range.selectNodeContents(command);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  } finally {
    copying = false;
    copy.disabled = false;
  }
});
