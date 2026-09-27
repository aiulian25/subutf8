// UI-10: selection in an ordered list, as in GNOME Files and other file managers. Click
// selects one item, Ctrl+click toggles one, Shift+click selects the range from the last
// clicked item, Ctrl+A selects all. The arrow keys, Home and End move the focus and select
// it; with Shift they extend the selection, with Ctrl they move without selecting, and Space
// toggles the focused item. `keys` are the items shown: changes apply to them only, so items
// picked in another folder stay picked.

const SELECT_ALL_KEY = "a";
const SPACE_KEY = " ";

const NAVIGATION = {
  ArrowDown: (index, last) => Math.min(index + 1, last),
  ArrowUp: (index) => Math.max(index - 1, 0),
  Home: () => 0,
  End: (index, last) => last,
};

function rangeBetween(keys, from, to) {
  const start = keys.indexOf(from);
  const end = keys.indexOf(to);
  if (start < 0 || end < 0) {
    return [to];
  }
  return keys.slice(Math.min(start, end), Math.max(start, end) + 1);
}

function modifiersOf(event) {
  return { toggle: event.ctrlKey || event.metaKey, extend: event.shiftKey };
}

export class Selection {
  constructor() {
    this.selected = new Set();
    this.anchor = null;
    this.focus = null;
  }

  has(key) {
    return this.selected.has(key);
  }

  get size() {
    return this.selected.size;
  }

  // The selected keys in list order.
  inOrder(keys) {
    return keys.filter((key) => this.selected.has(key));
  }

  // Drops keys that are no longer listed.
  keep(keys) {
    const listed = new Set(keys);
    this.selected = new Set([...this.selected].filter((key) => listed.has(key)));
    this.anchor = listed.has(this.anchor) ? this.anchor : null;
    this.focus = listed.has(this.focus) ? this.focus : null;
  }

  clear() {
    this.selected = new Set();
    this.anchor = null;
    this.focus = null;
  }

  // Puts the keyboard on an item without selecting it, as after opening a folder.
  placeFocus(key) {
    this.focus = key;
    this.anchor = key;
  }

  selectOnly(key) {
    this.selected = new Set([key]);
    this.anchor = key;
    this.focus = key;
  }

  // Replaces the selection among the items shown, keeping those picked elsewhere.
  replaceWithin(keys, chosen) {
    const shown = new Set(keys);
    const elsewhere = [...this.selected].filter((key) => !shown.has(key));
    this.selected = new Set([...elsewhere, ...chosen]);
  }

  click(key, keys, event) {
    const { toggle, extend } = modifiersOf(event);
    if (extend && this.anchor !== null) {
      const range = rangeBetween(keys, this.anchor, key);
      if (toggle) {
        this.selected = new Set([...this.selected, ...range]);
      } else {
        this.replaceWithin(keys, range);
      }
      this.focus = key;
      return;
    }
    if (!toggle) {
      this.replaceWithin(keys, [key]);
      this.anchor = key;
      this.focus = key;
      return;
    }
    this.toggle(key);
    this.anchor = key;
    this.focus = key;
  }

  toggle(key) {
    if (this.selected.has(key)) {
      this.selected.delete(key);
      return;
    }
    this.selected.add(key);
  }

  // Applies a key press; returns whether it was a selection key.
  press(keys, event) {
    const { toggle, extend } = modifiersOf(event);
    if (toggle && event.key.toLowerCase() === SELECT_ALL_KEY) {
      this.selected = new Set([...this.selected, ...keys]);
      return true;
    }
    if (event.key === SPACE_KEY && this.focus !== null) {
      this.toggle(this.focus);
      this.anchor = this.focus;
      return true;
    }
    const step = NAVIGATION[event.key];
    if (!step || keys.length === 0) {
      return false;
    }
    const index = keys.indexOf(this.focus);
    const target = keys[index < 0 ? 0 : step(index, keys.length - 1)];
    this.moveTo(target, keys, { toggle, extend });
    return true;
  }

  moveTo(target, keys, { toggle, extend }) {
    if (extend) {
      this.anchor = this.anchor ?? this.focus ?? target;
      this.replaceWithin(keys, rangeBetween(keys, this.anchor, target));
    }
    if (!extend && !toggle) {
      this.replaceWithin(keys, [target]);
      this.anchor = target;
    }
    this.focus = target;
  }
}
