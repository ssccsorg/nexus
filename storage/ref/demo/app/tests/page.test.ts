// What the deployment allows, held to the file the host reads.
//
// The board reads its own files, so no feature on it needs a device. The policy is the deployment's answer to that, and it
// is a header rather than a habit: a component added later that asks for a camera is refused by the deployment rather than
// prompting a reader to hand one over.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const headers = readFileSync(`${ROOT}public/_headers`, 'utf8');

/** The features a demonstration has no reason to ask for, and a reader has no reason to grant. */
const DENIED = [
  'camera',
  'microphone',
  'geolocation',
  'notifications',
  'display-capture',
  'clipboard-read',
  'clipboard-write',
  'usb',
  'serial',
  'hid',
  'midi',
  'payment',
];

test('the deployment denies every permission a demonstration does not need', () => {
  for (const feature of DENIED) {
    expect(headers, `${feature} is denied`).toContain(`${feature}=()`);
  }
});

test('and leaves the two the frame\u2019s own controls use', () => {
  // Fullscreen and autoplay are how the frame's window and the footage rows work, and neither reads a device.
  expect(headers).not.toContain('fullscreen=()');
  expect(headers).not.toContain('autoplay=()');
});
