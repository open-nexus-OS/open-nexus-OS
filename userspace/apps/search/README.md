# search — DSL app

App search in the `.nx` DSL (enterprise layout: `ui/pages` +
`ui/composables/search.store.nx`). The QUERY runs service-side
(`svc.bundlemgr.enumerate(query)` — bundlemgrd matches, stale replies drop
via the generation rule); a result tap launches through the launch authority
(`svc.ability.launch`).
