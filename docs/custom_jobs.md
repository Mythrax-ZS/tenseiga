# Custom jobs and palettes

## Custom jobs

Job ids **4500-4599** are custom jobs. They match rAthena's `JOB_CUSTOM_START`..`JOB_CUSTOM_END`
in the Tenseiga server. The client draws any id in that range as a player.

The Samurai (4500) is built in. Other jobs come from `System\customjobs.lua`, read from the
game data or an overlay:

```lua
CustomJobs = {
	[4501] = { sprite = "ronin", name = "Ronin" },
}
```

The sprite name is used the same way the stock jobs use theirs:

| What | Path |
| --- | --- |
| Body | `data\sprite\인간족\몸통\{남,여}\ronin_{남,여}.spr/.act` |
| Weapons | `data\sprite\인간족\ronin\ronin_{남,여}{weapon}.spr/.act` |
| Body palettes | `data\palette\몸\ronin_{남,여}_{n}.pal` (below) |

A job in the range that the file doesn't list is drawn as a Novice.

## Body palettes (clothes colours)

Clothes colour `n` loads `data\palette\몸\{job sprite}_{sex}_{n}.pal`, or
`data\palette\도람족\body\...` for Doram jobs (`jobs::body_palette`).

- **Numbering.** Palettes are per job, as named, and any number works. A server's custom
  palette pack only has to name its files this way. A number a job has no file for shows the
  sprite's own colours.
- **Server limit.** The server decides which numbers can be set (`max_cloth_color` in
  rAthena's `conf/battle/client.conf`).
- **Indexed sprites only.** Palettes recolour palette-indexed sprites. Truecolor bodies
  keep their colours, which includes the Samurai's current art.

`--offline --job <sprite> --dye <n>` shows a job wearing palette `n` without logging in.
