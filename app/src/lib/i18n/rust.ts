// What the engine and the Tauri commands actually send (English). tm()
// matches a message against these to find its key, then shows that key in
// the current language. Keep in step with the Rust strings.

import type { Key } from './en';

export const RUST: Partial<Record<Key, string>> = {
  'err.codeFormat': 'A code has 6 letters and numbers.',
  'err.onlyListenerPrivate': 'Only the listener can be private.',
  'err.waitHandover': 'Wait for the handover to finish.',
  'err.becomeListener': 'Couldn’t become the listener: {reason}',
  'err.listenerGone': 'That listener is no longer on the network.',
  'err.listenerPrivate': 'That listener is private. Join it with its code.',
  'err.stoppedListening': '{name} stopped listening.',
  'err.alreadySends': '{name} already sends its audio to this device.',
  'err.deviceGone': 'That device is no longer on the network.',
  'err.notSending': '{name} isn’t sending to this device.',
  'err.privateHandover': 'A private listener can’t hand over yet. Make it public first.',
  'err.didntTakeOver': '{name} didn’t take over. This device is still the listener.',
  'err.wrongCode': 'That code didn’t work. Check it and try again.',
  'err.noSuchCode': 'No listener has that code. Check it and try again.',
  'err.loopback': 'Joining would play your own audio back to you.',
  'err.privateNeedsPro': 'Making a listener private needs Aonin Pro.',
  'err.makeNeedsPro': 'Make listener needs Aonin Pro.',
  'err.invalidKey': 'That license key isn’t valid. Check it and try again.',
  'err.keyNeedsNewer': 'This license key needs a newer version of Aonin.',
  'err.enterKeyFirst': 'Enter your license key first.',
  'err.proUnavailable': 'Aonin Pro isn’t available for this version yet. Try again later.',
  'err.noUpdate': 'No update is ready.',
  'err.wifiCancelled': 'Wi-Fi setting not changed (the password prompt was cancelled).',
};
