//=============================================================================
// DisabledPlugin.js
//=============================================================================
/*:
 * @target MZ
 * @plugindesc Disabled plugin; status:false in plugins.js so it is ignored.
 * @author dk-doctor fixture
 *
 * @command unused
 * @text Unused
 *
 * @help DisabledPlugin.js
 * Control: status:false -> not in load_order, its @command is not registered,
 * and it is not read (collect skips disabled plugins).
 */

(function () {
  "use strict";
})();
