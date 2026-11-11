# A project from another board

`projects.json` and `scenario/ecu.txt` are one project, copied from a demonstration built on the same frame whose subject
is a different domain: an ABS controller on bare metal, four wheels recording their pulses, two brake controllers their
pressure, and no calculation anywhere in it. This board's other projects are an ESG reporting quarter and one sample
record, so the copy is what shows that the frame carries a second domain without any of them changing.

It is kept as a board of its own so that the copy is a copy between equals: `app/tools/sync-project.mjs` reads a board and
writes a board, so the project travels into this demonstration and back with the two paths the other way round.

```bash
# take it into this demonstration
node storage/ref/demo/app/tools/sync-project.mjs --from storage/ref/demo/shared/ecu \
  --id ecu --into storage/ref/demo/app

# give this demonstration's version back
node storage/ref/demo/app/tools/sync-project.mjs --from storage/ref/demo/app \
  --id ecu --into storage/ref/demo/shared/ecu --replace
```

The run's header carried the command that produced the source volume, and that line is removed here because this
repository publishes no product name. Everything else travels as it was written.

Two things it demonstrates beyond a second domain. Its records are observations and nothing concludes them, so the board
draws no chain for it and the evidence panel does not stand for it: a surface that opened onto nothing would be a control
promising a view the project does not have. And its vocabulary is inherited rather than declared, which is why the
application's brand carries the three words.

Nothing in it is a real vehicle. The project is a fixture.
