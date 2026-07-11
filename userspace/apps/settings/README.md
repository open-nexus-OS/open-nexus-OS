# settings — DSL app

The system settings app (design handoff "OS Settings Window") in the `.nx`
DSL. Enterprise layout: `ui/pages` + `ui/components/settings/SectionChip.nx`
+ `ui/composables/settings.store.nx`. `bundle_type = "settings"` admits the
`nexus.permission.SETTINGS` cap; theme writes go through `svc.settings.set`
— the app-host routes the presentation key to windowd (live apply + persist).
Compact chip navigation stands in for the handoff sidebar until floating
windows negotiate wider defaults.
