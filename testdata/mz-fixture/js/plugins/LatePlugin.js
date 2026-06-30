//=============================================================================
// LatePlugin.js
//=============================================================================
/*:
 * @target MZ
 * @plugindesc Plain plugin that happens to load after DependentPlugin.
 * @author dk-doctor fixture
 *
 * @help LatePlugin.js
 * Control: declares no order constraints itself. It is only the *target* of
 * DependentPlugin's @orderAfter, and its later position triggers that
 * violation -- LatePlugin itself is not flagged.
 */

(function () {
  "use strict";
})();
