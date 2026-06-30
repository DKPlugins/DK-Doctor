//=============================================================================
// DependentPlugin.js
//=============================================================================
/*:
 * @target MZ
 * @plugindesc Depends on a base that is not installed and is ordered wrong.
 * @author dk-doctor fixture
 *
 * @base GhostBase
 * @orderAfter LatePlugin
 *
 * @help DependentPlugin.js
 * Planted bugs:
 *  - @base GhostBase: GhostBase is absent from plugins.js -> missing-base.
 *  - @orderAfter LatePlugin: LatePlugin loads AFTER this plugin in plugins.js,
 *    so the @orderAfter requirement (LatePlugin must load earlier) is violated
 *    -> plugin-load-order.
 */

(function () {
  "use strict";
})();
