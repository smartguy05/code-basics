# Debug launches, project-scoped run configurations, app icon

## Reported (2026-09-02)

> There are some projects in the oneflight solution that need to be run with the
> debugger attached so that the proper redis stream is used. Additionally there
> are Rider configurations that aren't being properly brought in and when I try
> to save a configuration the environment variables I set are not persisting.
> Also configurations should be unique per project.

Follow-up: "small bug, the installed app doesn't have an app icon".

## Acceptance criteria

1. A **Debug** action beside Run launches .NET and Node application
   configurations under a real debug adapter, so debugger-dependent behaviour
   (the Redis stream selection) is active. Run behaviour is unchanged.
2. Different projects may be debugged concurrently; one configuration has at
   most one live Run **or** Debug generation.
3. Rider imports get project-scoped identities, so two projects may both have a
   `Development` configuration without overwriting each other or losing saved
   environment variables.
4. The installed Windows app shows its icon in the title bar and taskbar.

## Scope boundary

Launch-attached debugging only: console output, lifecycle state, Stop.
**No** breakpoints, stepping, stacks, watches or variables — that is future UI
work over the already-pure `cb_core::dap::breakpoints`/`positions` layers.
