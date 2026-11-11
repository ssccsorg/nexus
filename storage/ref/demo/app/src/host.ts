// The host this deployment stands over.
//
// The board and the runs it names are files this bundle shipped with, so there is no shell, no room, and no feed to
// follow. Saying so is what keeps the frame from dialling the demonstration's loopback address for a server a static
// deployment does not have.

import { NO_SHELL, type Host } from '@ssccsorg/skene-frame';

export const offTheShelf: Host = {
  reachable: false,
  static: true,
  ask: () => Promise.reject(new Error(NO_SHELL)),
  listen: async () => () => {},
};
