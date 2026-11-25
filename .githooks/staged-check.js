#!/usr/bin/env node
// Helper for .githooks/pre-commit. Decides whether the staged files need the Rust checks.
// Reads nothing from stdin. Works from any directory inside the repo.
//
// Output on stdout (the hook reads this line):
//   skip    nothing relevant is staged, the commit is not checked
//   check   run cargo fmt --all -- --check, then cargo clippy --workspace --all-targets -- -D warnings
//
// The hook always checks the whole workspace (decision 0043): cargo unifies dependency features over the selected
// packages, so a `-p` selection would build its own copy of the dependencies.
//
// Environment:
//   RIMELIGHT_HOOK_FILES="a b c"  use these paths instead of the staged list (dry runs and tests)
//   RIMELIGHT_HOOK_DRY=1          print the staged list and the reasoning on stderr
'use strict';

const path = require('path');
const { spawnSync } = require('child_process');

const DRY = process.env.RIMELIGHT_HOOK_DRY === '1';
const RELEVANT_EXTENSIONS = ['.rs', '.slint'];
const CARGO_FILE_NAMES = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'];

function note(message) {
  if (DRY) process.stderr.write(`staged-check: ${message}\n`);
}

function die(message) {
  process.stderr.write(`staged-check: ${message}\n`);
  process.exit(1);
}

function stagedFiles() {
  if (process.env.RIMELIGHT_HOOK_FILES !== undefined) {
    return {
      source: 'RIMELIGHT_HOOK_FILES',
      files: process.env.RIMELIGHT_HOOK_FILES.split(/\s+/).filter(Boolean),
    };
  }
  const diff = spawnSync('git', ['diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z'], {
    cwd: __dirname,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  if (diff.status !== 0) die(`git diff --cached failed: ${diff.stderr.trim()}`);
  return { source: 'git index', files: diff.stdout.split('\0').filter(Boolean) };
}

function isRelevant(file) {
  const normalised = file.split(path.sep).join('/');
  if (RELEVANT_EXTENSIONS.some((ext) => normalised.endsWith(ext))) return true;
  const base = normalised.slice(normalised.lastIndexOf('/') + 1);
  return CARGO_FILE_NAMES.includes(base) || normalised === '.cargo/config.toml';
}

const staged = stagedFiles();
note(`staged files (${staged.source}): ${staged.files.length ? staged.files.join(', ') : '(none)'}`);
if (staged.files.some(isRelevant)) {
  note('Rust, Slint or Cargo files staged, checking the whole workspace');
  console.log('check');
} else {
  note('no .rs, .slint or Cargo files, skipping');
  console.log('skip');
}
