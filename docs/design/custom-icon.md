# Custom icon compatibility

## Contract detail

Custom icons and their configuration schema/editor are removed by the [approved icon decision](../reviews/visual-decisions.md). Only Icon and fixed [Summary](icon-summary.md) remain. `taskq tray --icon FILE` returns a clear removed-feature error before reading FILE or changing settings.

A saved `view: custom` resolves to Summary. Other tray settings survive load/save; the obsolete icon configuration is not validated or rendered. A subsequent tray preference save discards its `icon` field; reverting the code cannot restore that field. Custom History time ranges remain supported under [M6](menu.md#m6-cadence).
