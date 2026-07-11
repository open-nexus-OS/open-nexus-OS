# chat — DSL app

A conversation view in the `.nx` DSL (enterprise layout: `ui/pages` +
`ui/composables/chat.store.nx`). Messages are LOCAL (append + echo effect)
until the app-to-app transport lands — the manifest keeps the `chat.Send`/
`chat.Receive` exports declared for the TASK-0081 C2 exports channel.
Known gap: the visible history is height-bounded (no DSL scrolling yet).
