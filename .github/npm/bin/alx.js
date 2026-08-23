#!/usr/bin/env node

const path = require("path");
const { spawn } = require("child_process");

const FALLBACK_HINT = [
  "",
  "Prebuilt binaries are published for:",
  "  - Linux x64/arm64 (glibc)",
  "  - macOS x64/arm64",
  "  - Windows x64",
  "",
  "Fallback install methods:",
  "  - cargo install agent-lx-music",
  "  - https://github.com/Xuepoo/agent-lx-music/releases",
].join("\n");

function fail(message) {
  console.error(`alx: ${message}`);
  process.exit(1);
}

function isMusl() {
  if (process.platform !== "linux") return false;
  try {
    return !process.report.getReport().header.glibcVersionRuntime;
  } catch {
    return true;
  }
}

function resolvePlatformPackage() {
  switch (`${process.platform}:${process.arch}`) {
    case "linux:x64":
      return { pkg: "agent-lx-music-linux-x64-gnu" };
    case "linux:arm64":
      return { pkg: "agent-lx-music-linux-arm64-gnu" };
    case "darwin:x64":
      return { pkg: "agent-lx-music-darwin-x64" };
    case "darwin:arm64":
      return { pkg: "agent-lx-music-darwin-arm64" };
    case "win32:x64":
      return { pkg: "agent-lx-music-win32-x64" };
    default:
      return null;
  }
}

function resolveBinary(target) {
  try {
    const pkgDir = path.dirname(require.resolve(`${target.pkg}/package.json`));
    const binary = process.platform === "win32" ? "alx.exe" : "alx";
    return path.join(pkgDir, "bin", binary);
  } catch {
    fail(
      `optional dependency "${target.pkg}" is not installed\n${FALLBACK_HINT}`,
    );
  }
}

const target = resolvePlatformPackage();
if (!target) {
  fail(
    `unsupported platform ${process.platform}-${process.arch}\n${FALLBACK_HINT}`,
  );
}

if (isMusl()) {
  fail(
    `unsupported C library: musl detected (only glibc builds are published)\n${FALLBACK_HINT}`,
  );
}

const binary = resolveBinary(target);
const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });

child.on("error", (err) => {
  if (err.code === "ENOENT") {
    fail(
      `binary not found at ${binary}; reinstall with "npm install -g agent-lx-music"`,
    );
  }
  if (err.code === "EACCES") {
    fail(`binary at ${binary} is not executable; fix permissions or reinstall`);
  }
  fail(err.message);
});

child.on("close", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
