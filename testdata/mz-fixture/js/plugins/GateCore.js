//=============================================================================
// GateCore.js
//=============================================================================
/*:
 * @target MZ
 * @plugindesc Owns the plugin gate switch and registers a command.
 * @author dk-doctor fixture
 *
 * @param GateSwitch
 * @text Gate Switch
 * @desc Switch the plugin drives at runtime to open/close the gate.
 * @type switch
 * @default 3
 *
 * @command openGate
 * @text Open Gate
 * @desc Opens the gate (sets the gate switch ON).
 *
 * @help GateCore.js
 * The plugin toggles GateSwitch (#3) at runtime, so any page gated on it is
 * driven by the plugin (Tier A: stuck-autorun must NOT flag it).
 */

(function () {
  "use strict";
  PluginManager.registerCommand("GateCore", "openGate", function () {
    $gameSwitches.setValue(3, true);
  });
})();
