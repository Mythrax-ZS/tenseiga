# Parity checklist

Server packets rAthena can send at PACKETVER 20250402 that the client does not handle yet,
found by comparing `ro-proto/src/generated.rs` with the opcodes in `ro-net` and `ro-proto`.
Ticked items are handled and tested in game against the local server. While playing, the
client's log names every packet it doesn't handle yet (`unhandled packet`, once per map session).

## Scripting (NPC script commands)
- [x] ZC_SOUND (`soundeffect`), ZC_PLAY_NPC_BGM (`playBGM`)
- [x] ZC_COMPASS (`viewpoint`: minimap marks)
- [x] ZC_NPCSPRITE_CHANGE (`setnpcdisplay`)
- [x] ZC_CHANGE_DIRECTION; server directions (0 north) converted to sprite directions (0 south): standing units faced the wrong way
- [x] ZC_DIALOG_TEXT_ALIGN / WINDOW_SIZE / WINDOW_POS / POS2 (ZC_SAY_DIALOG2 / WAIT_DIALOG2 are never sent by rAthena)
- [x] ZC_MSG_COLOR, ZC_MSG_VALUE, ZC_MSG_SKILL, and every ZC_MSG id (was: only two)
- [x] ZC_SPECIALPOPUP (System/spopup.lub: chat and/or floating banner), ZC_SKILLMSG
- [x] ZC_EQUIPMENT_EFFECT
- [x] ZC_REMOVE_EFFECT (`removespecialeffect`): stops a script effect still playing on the unit. Tested: Lord of Vermillion cut off 700 ms in
- [x] ZC_NOTIFY_EFFECT3 (0x0284 / 0x0b69: effect plus green heal numbers)
- [x] ZC_NPC_MARKET_OPEN / PURCHASE_RESULT (market shops)
- [x] ZC_NPC_BARTER_MARKET_ITEMINFO / EXPANDED_BARTER (barter shops)
- [x] Stylist (openstylist: hair, hair colour, clothes colour, live preview)
- [x] ZC_DRESSROOM_OPEN (opendressroom: preview only)
- [x] ZC_ADD_SKILL, ZC_SKILLINFO_UPDATE2, ZC_SKILLINFO_DELETE
- [x] Random enchant (ZC_UI_OPEN_V3 + ZC_RESPONSE_ENCHANT) from the official enchantlist.lub (32-bit bytecode widened and run)
- [x] Item reform (ZC_OPEN_REFORM_UI / ITEM_REFORM_ACK) from itemreformsystem.lub
- [x] Failed item adds (full inventory, overweight) now say why
- [x] Reputation window (ZC_REPUTE_INFO / OPEN, names from contentdata
epute*.bson)
- [x] Item merge window (mergeitem): tested with three Usable Power Box 1 (13826). rAthena merges only stackable, non-Cash items with a unique id (item_db `UniqueId`, each one obtained its own stack); the merged stack takes its new total from ZC_ACK_MERGE_ITEM (it kept the old amount). The stock Mergician stands on itemmall, which iRO and kRO data lack; a copy elsewhere that runs `mergeitem` works
- [x] ZC_RANDOM_COMBINE / RANDOM_UPGRADE_ITEM (Laphine synthesis + upgrade windows, recipes from lapine*.lub merged across iRO/kRO; ZC_CHANGE_ITEM_OPTION 0x0b43)

## Items and skills
- [x] ZC_ACK_ITEMIDENTIFY (Magnifier) + ZC_AUTORUN_SKILL (all itemskill items)
- [x] ZC_REPAIRITEMLIST / ACK_ITEMREPAIR (Weapon Repair list and result)
- [x] ZC_ACK_WEAPONREFINE message, ZC_NOTIFY_WEAPONITEMLIST (the Weapon Refine list window)
- [x] ZC_ACK_REMEMBER_WARPPOINT (Warp Portal memo)
- [x] ZC_EQUIP_ARROW, ZC_ACTION_FAILURE, ZC_ACK_ADDITEM_TO_CART, ZC_NOTIFY_MAPINFO, ZC_EQUIPITEM_DAMAGED
- [x] ZC_ATTACK_RANGE, ZC_OVERWEIGHT_PERCENT, ZC_MAPPROPERTY_R2 (PvP/GvG: click enemy players to attack, Shift when the map requires it)
- [x] ZC_CARTOFF
- [x] ZC_SPIRITS (spheres circling the unit)
- [x] ZC_SPIRITS2 / SPIRITS_ATTRIBUTE / SOULENERGY, ZC_MILLENNIUMSHIELD (orbs coloured by kind)
- [x] ZC_TARGET_SPIRITS (Enchanting Shadow orbs, dark violet)
- [x] ZC_DISPEL (cast cancel), ZC_HIGHJUMP (slide); ZC_NOTIFY_SKILL_POSITION unused by rAthena
- [x] ZC_BLADESTOP (both units held in place): tested with a Monk's Root against an Orc Warrior
- [~] ZC_COMBODELAY (logged only) and Monk combos: Chain Combo after Triple Attack did nothing in game, cause not found yet. Test with ordinary stats (the window is 1000 - 4 AGI - 2 DEX ms, never under the attack motion) and a target that survives the hit, and with RUST_LOG=tenseiga=debug ("combo window", "use skill", "skill hit")
- [x] ZC_NOTIFY_BIND_ON_EQUIP (chat notice)
- [x] ZC_CHANGE_ITEM_OPTION (0x0b43, Laphine upgrade); ZC_UPDATE_CARDSLOT not sent by rAthena
- [x] ZC_ACK_TAKEOFF_EQUIP_ALL ("Take off all" button per equipment tab), ZC_MOVE_ITEM_FAILED
- [x] ZC_INVENTORY_TAB (own fourth "fav" tab; the server flag is PlaceETCTab); ZC_INVENTORY_END is a no-op marker
- [x] ZC_EXTEND_BODYITEM_SIZE (+ ACK / MSGBOX): slot count, + button, confirmation

## World
- [x] ZC_MVP (the MVP effect over the killer and "… is the MVP!"), ZC_MVP_GETTING_ITEM (the reward in chat): tested with an @monster MVP. ZC_MVP_GETTING_SPECIAL_EXP and ZC_THROW_MVPITEM are handled but never sent by rAthena at this PACKETVER: clif_mvp_exp drops the packet from 2013-12-23 (a cyan chat line instead, with battle config `mvp_exp_reward_message`), and nothing calls clif_mvp_noitem (a reward that doesn't fit is a failed add and drops to the floor)
- [x] ZC_NOTIFY_POSITION_TO_GROUPM / GUILDM (party members off-screen on the minimap)
- [x] ZC_NOTIFY_MEMBERINFO_TO_GROUPM
- [x] ZC_NOTIFY_MAPINFO, ZC_BOSS_INFO (Convex Mirror: chat + minimap mark)
- [x] ZC_UPDATE_MAPINFO (setcell/setwall change client pathing), ZC_MONSTER_INFO (Sense window)
- [x] ZC_NOTIFY_MAPPROPERTY2 (battleground map type; consumed, nothing to draw)
- [x] ZC_CHANGE_DIRECTION
- [x] Weather (the snow, sakura, leaves, clouds, clouds2 and fog mapflags, GM @snow / @sakura / @leaves / @clouds / @clouds2 / @fog; EF_RAIN too, which tenseiga-athena brings back as the rain mapflag and @rain): the special effects rAthena sends on entering the map turn on snow, petals, maple leaves, drifting clouds or fog banks, and whenever a map's weather changes the server sends all that is still on at once, which the client takes as the whole of it (so @sakura off under snow leaves the snow). Turning the last kind off sends nothing (the original keeps it until the map changes); the GM's "...has stopped" message, and @clearweather's, clears it for them. Tested online with @snow and @sakura. The fireworks mapflag (@fireworks; EF_POKJUK, EF_THROWITEM2, EF_POKJUK_SOUND) sets off fireworks around the camera while it is on, as the original draws them in code: rockets going up and bursting into the original's pokjuk_a..d art (streaks, spray, gold glitter, twinkles) in random colours, every half second to two seconds with now and then a volley, each bursting to effect\itempokjuk.wav; "Fireworks have ended." stops them
- Beyond the original: cloud shadows (Settings > Graphics > Cloud shadows, HD-2D look): soft patches of shade drift over outdoor maps, cast along the sun from a cloud layer 120 cells up (value noise in scene.wgsl and sprite.wgsl), taking only the sun's share of the light, so they fade at night and in shade; clouds, rain, fog and snow bring more of them, and deeper
- Beyond the original: rain soaks the ground (HD-2D look): over its first half minute the ground darkens and turns glossy, the sun glinting off it, and puddles gather in the low spots of a noise field, holding the (greyed) sky, with rings spreading where the drops land; rings spread over water too; it dries over a couple of minutes after the rain. Models (told apart by their single lightmap texel) darken but get no puddles
- Beyond the original: thunderstorms: heavy rain (0.6 or more) brings a lightning strike every 12 to 45 seconds, two or three quick cold flickers lighting the whole outdoor map (its ambient and sun light, the exposure), then thunder (effect\ef_thunderstorm.wav) 0.4 to 2.5 seconds later
- Beyond the original: trees, bushes, grass, reeds and flowers sway in the wind (Settings > Graphics > Swaying trees): map models are told foliage by their file names (나무, 풀, 덤불, 꽃, 갈대, 야자, 넝쿨, tree, bush, grass, flower, palm, the 나무잡초꽃 folder; not wooden things or buildings), each vertex given how far it sways by its height up the model, so trunks stay rooted; in scene.wgsl they lean downwind and sway back in gusts rolling across the map, leaves fluttering, blowing harder in rain, clouds and snow. Ray-traced shadows keep the still shapes
- Beyond the original: heat haze (Settings > Graphics > Heat haze, HD-2D look): over desert and volcanic maps by day the air shimmers, rising, more the further off (post.wgsl, a noise-driven wobble of the picture); rain and snow cool it off, and it fades at night
- Beyond the original: icy water (Settings > Graphics > Icy water, 0 open .. 1 frozen over; default 0.6): on snowy maps (Lutie and its fields, Manuk, the ice castle) a shelf of ice reaches out from the banks, ragged at its edge, and floes drift slowly on the water with dark channels between; all the way up the water freezes over. The ice is a slab standing on still water (the view ray is traced through it): floes show their sides and hide the water behind them, and throw shadows on it. Pale blue ice dusted with snow, faint cracks, bright rims, a sheen of the sky (water.wgsl `ice_cover`)
- Beyond the original: night lights (Settings > Graphics > Night lights): as night falls outdoors, building windows glow warm, about six in ten, flickering gently like lamplight. Windows are found by shape when a texture loads (windows.rs): regions of like colour that are compact rectangles or arches of a sensible size, inside the texture, neutral or cool, set off by a frame, grouped across mullions, and not one of many alike (slates, stones); the mask (glass, and a number per window so it lights whole) is sampled on near-vertical model faces only, never on props (small models; banners, lamps, signs, tents by name) or thin pieces (poles, trim), and on green maps (forest, temperate, blossom, highland, coastal) swarms of fireflies come out unless it rains, a dozen points each wandering and blinking
- Beyond the original: silhouettes (Settings > Graphics > Silhouettes): we, party members and the target show through walls and buildings in front of them as a soft shape (blue, green, red) brighter at its edge: their sprites drawn again where the scene is nearer (depth test Greater), their depth pulled three cells further toward the camera so the ground at their feet doesn't count
- Beyond the original: footsteps (Settings > Graphics > Footsteps): what the ground is made of is read per cell from its GND texture names (초원 grass, 흙 soil, 맨땅/모래 sand, 눈바닥 snow, 도시 paving...), and each step a player or monster takes leaves a print (sole and heel, left and right) in snow, sand, or dirt turned to mud by rain, fading over seconds, and kicks up a little dust off dry dirt and sand
- Beyond the original: photo mode (Ctrl+P in Shortcut Settings; Esc leaves): the world's time stands still (actors, effects, weather, water, swaying trees, the sun), the interface steps aside for a small panel (H hides it), and the camera goes free: WASD or the arrows move it, Q and E down and up, right-drag turns it, the wheel zooms 30 to 1600 units, tilt 4 to 89 degrees; depth of field, exposure, saturation, warmth, vignette and field of view to hand; Save photo or Print Screen saves the picture without the panel
- Beyond the original: the sky's clouds are a layer overhead seen in perspective (sky.wgsl): domain-warped gradient noise with cauliflower puffs, shrinking toward the horizon instead of stretching across wide screens, lit through themselves toward the sun (bright tops, darker bellies, silver linings facing it), with thin cirrus streaks above; distant detail fades rather than shimmers. The sky is kept nearly sharp under depth of field
- Beyond the original: the sky follows the weather (`Renderer::weathered_sky`): rain greys it over to a dark, heavy deck of cloud with darker and lighter masses and ragged scud racing below; snow greys it paler; drifting cloud and fog haze it a little; a lightning flash lights the clouds from within; overcast hides the stars. The haze distant scenery fades into and what wet ground mirrors take the same weathered sky
- Beyond the original: weather is drawn HD-2D style (crates/ro-render/src/weather.wgsl): thousands of particles placed on the GPU, in tiles repeated over all the camera sees (any window shape, any zoom), still in the world as it moves, lit by the map (dim at night), hidden behind walls and indoors, with /effect. Rain, snow, clouds and fog cloud the sky over (softer shadows, dimmer sun). Settings > Graphics > Weather adds ambient weather by climate (the default): snow on snowy maps, petals in Amatsu and Louyang, leaves in forests, fog in Niflheim, and on temperate, coastal, forest and highland maps a twenty-minute shower about one spell in six, on each map's own clock-driven schedule; `--weather snow,rain` forces it for captures
- [x] ZC_COUPLENAME (Call Partner notice)
- [x] ZC_NOTIFY_ACTORINIT (consumed); ZC_SKILL_SCALE not sent by rAthena
- [x] Taming (ZC_START_CAPTURE / TRYCAPTURE_MONSTER)
- [x] ZC_FEED_PET, ZC_FEED_MER (chat result)
- [x] ZC_PET_ACT (pettalktable.xml lines in chat, emotions)
- [x] ZC_SUMMON_HP_INIT / UPDATE, ZC_HO_PAR_CHANGE2, ZC_HOSKILLINFO_UPDATE (summon bar tested with ABR Battle Warrior)
- [x] Elementals (ZC_EL_INIT, ZC_EL_PAR_CHANGE): companion window tab + HP bar
- [x] 3D monsters (Granny .gr2: Sword / Bow / Soldier Guardians, Emperium, guild flag, treasure box): models, wavelet textures and move / attack / damage / dead clips, skinned on the CPU (seen in Thor's Volcano); were invisible

## Social
- [x] Guild emblems in the guild window: BMP, GIF or PNG (with alpha) up to 256x256 from the Emblem folder, uploaded to and downloaded from the web server (/emblem/*, CZ_REQ_ADD_NEW_EMBLEM) as 2019+ clients do; 24x24 BMPs over the game connection when there is no web server
- [x] Guild emblems beside player names (guild id from spawn packets, fetched per guild)
- [x] ZC_CHANGE_GUILD
- [x] ZC_GUILD_POSITION (rank title under names updates live)
- [x] Guild alliances and enemies (request, answer, list in the guild window)
- [x] Guild disorganize
- [x] ZC_CONFIG, ZC_PARTY_CONFIG, party options: Game Options window
- [x] ZC_PARTY_JOIN_REQ_ACK, ZC_ITEM_PICKUP_PARTY
- [x] Store search (Universal Catalog: search, pages, click to locate)
- [x] ZC_ACK_OPENSTORE2
- [x] Ranking window (ZC_ACK_RANKING2 + names by char id; /blacksmith /alchemist /taekwon /pk)
- [x] ZC_CONGRATULATION: the wedding ceremony's effect (EF_COLORPAPER, also `@effect 347`), coloured paper showering down on the couple to effect\wedding.wav, drawn in code as the original does; tested with @effect. ZC_DIVORCE: a chat notice
- [x] ZC_BAN_LIST

## Found while testing
- [x] GM look: the GM sprite (운영자) and yellow names, from clientinfo.xml-style lists (config gm_aids / yellow_aids) or switched for everyone by the GM (@gmlook, kept in the name font: ZC_NOTIFY_FONT, ZC_ACCEPT_ENTER3, unit packets)
- [x] Drop light pillars (ZC_ITEM_FALL_ENTRY5 drop effect): cards fall with the pink pillar and its sound, other modes blue / yellow / purple / green / red; gone when the item is picked up
- [x] Traps (Hunter and 3rd-job trap units) are the original's trap models (data\model\외부소품\트랩*, event\3차트랩_*) on their cell, not a looping effect; Fire Pillar's waiting unit shows its red column. Others' hunter traps stay hidden, as rAthena's renewal traps_setting sends them as invisible units
- [x] Identified items kept their unidentified icon (icons were cached per item id); unidentified looks are now kept apart
- [x] NPCs drawn with a monster sprite (object type 12, e.g. the Twin Flames fox Kohaku) were not clickable and had no name
- [x] `specialeffect` (ZC_NOTIFY_EFFECT2): 110 EF_* ids matched to STR files, plus the most-used code-drawn ones (heal, grand cross, steal, bash, ...) shown with their skill effect
- [x] Hit sparks: a normal hit landing on a monster bursts four flecks of effect\pok3.tga from it (EF_HIT1, as roBrowser spawns it; `--fx hit` previews it on the player)
- [x] Screen quake: the camera jumps a sixth of a cell side to side (and half that up and down) every 33 ms during Meteor Storm, Earth Spike, Heaven's Drive, Acid Terror, Acid Demonstration, Pressure, Magic Crasher, Falcon Assault, Gravitation Field, monster Earthquake and Dragon Fear, and the matching specialeffect ids (EF_SCREEN_QUAKE among them), with roBrowser's delays and lengths; Settings > Game > Effects turns it off. roBrowser flips a tenth of a cell every frame, which blurs away at high frame rates; measured in game here, @effect 666 moved the view about 25 px each jolt at 3440x1440 at a third of a cell, which looked twice as strong as the original, so it was halved
- [x] NPC classes whose sprite iRO lacks (e.g. 10243 4_m_drzonda01, Zonda teleporters) get a stand-in body so they stay usable
- Note: `setnpcdisplay` to a monster class makes rAthena send class 0 (a Novice); the original client shows the same
- [x] ZC_ACK_REQNAMEALL: party, guild and rank under player names
- [x] Names on hover: a monster's name shows under it while the cursor is over it (players and NPCs always show theirs), and a floor item's as "Jellopy: 3 ea." (msgstringtable 183; the unidentified name when unidentified)
- [x] Experience messages: monster EXP shows in chat only with Settings > Game > Chat "Experience messages" (the original's chat option, off by default); quest rewards always show, as "You got N Base EXP." (msgstringtable 487/488)
- [x] Shortcut Information window (Esc menu; the original's menu entry, msgstringtable 3880): every keyboard, mouse and gamepad shortcut, with the battle-mode rows as configured and the controller's own button icons
- [x] Insert sits down or stands up, as in the original
- [x] Quest window location: a quest whose quest info file gives NpcNavi / NpcPosX / NpcPosY shows "Location: map (x, y)" with a Navigate button that opens the navigation route there, as the original's quest window does
- Beyond the original (it has no controller support): controllers with Xbox, PlayStation or Nintendo button icons (from the pad, or Settings > Controls), drawn wherever a button is named. Hold RT / LT for the shortcut row on show / the next; the face buttons, D-pad and R3 fire its nine slots, and a cross bar shows the row while the trigger is held. In battle mode both triggers reach a third row and skills fire at once on the target (the nearest monster without one), support skills on the targeted player or yourself, ground skills under the target
- Beyond the original: controller tuning in Settings > Controls > Controller. Radial deadzone per stick with a live preview, full-tilt point (outer deadzone), stick swap, camera turn and tilt speed, response curve and inversion, trigger press point, touchpad pointer speed, and a reset. gilrs's own deadzone filter is off so these are the only ones applied
- Beyond the original: controller feedback. Rumble when hit (scaled by the share of HP lost), on our critical hits, level up, death, item pickup, and a heartbeat below a quarter HP; for skills, a hum building while we chant, a pulse per hit as a damaging skill lands, a rolling rumble for ground skills, a swell for support (gentler when another player buffs us), a zap for a hex on a monster, and a double tick when a skill fails; Settings > Controls > Vibration. Xbox and other pads through gilrs; DualSense and DualShock 4 through their HID reports (light bar shows HP); Switch Pro Controller as HD rumble (both bands, frequency and amplitude); a USB DualSense plays the cues as waveforms on its haptic actuators (its audio channels 3 and 4). The PlayStation touchpad moves the pointer, clicks (two fingers: right-click), swipes rows and zooms. Built from the Linux hid-playstation and Switch protocol documentation; not yet tried on hardware
- [x] ZC_EQUIPMENT_EFFECT: hat effects (145 from hateffectinfo), following the wearer
- [x] Equipment switch set (Switch tab: add, remove, swap; ZC_EQUIPSWITCH_LIST/ADD/REMOVE/REPLY)
- [x] Level aura for players reported at level 99+ (clevel from the spawn packets; EF_LEVEL99 ribbons, EF_LEVEL99_2 ground star, EF_LEVEL99_3 sparkles, code-drawn after roBrowser's reconstruction); /aura simplifies it, /aura2 turns it off. rAthena's `max_lv` / `aura_lv` / `client_limit_unit_lv` choose who gets it
- [ ] The higher auras (EF_LEVEL99_150, the 4th-job orbs EF_LEVEL99_ORB1/2): when the original shows them is unknown
- [x] Emotion List (Alt+L, /emo): every emote with its picture and command, click to show it. The emotion sprite's actions are not in ET_* order (`emotes::action`, as roBrowser's table), which also fixed the emotes drawn over heads
- [x] Macro window (Alt+M, "Shortcut List"): what Alt+1..Alt+0 say, the first ten emotes by default; any emote, phrase or / command. The minimap toggle moved to Ctrl+Tab, as in the original
- [x] Battle mode (/bm, /battlemode, msgstringtable 785-796): Q ~ O, A ~ L and Z ~ > use the second, third and fourth shortcut rows; Enter or Space opens the chat and sending a line gives the keys back. Outside it, typing over the game goes to the chat and the chat keeps the keyboard, while F1..F12 and Alt shortcuts still work
- [x] Settings > Battle mode: on/off, Space opens the chat (or uses a slot), rows on show, and the keys for rows 2-4 (any nine keys each; Default keys restores Q ~ O, A ~ L, Z ~ >)
- [x] Basic Skill refusals (ZC_ACK_TOUSESKILL for NV_BASIC) show the original's messages (MsgStringTable 159-165, 383), e.g. emotes faster than rAthena's one per second or two
- [x] Shortcut bar rows: F12 shows one to four rows or hides the bar (the original's extend), and dragging the bar's top edge resizes it to one to four rows like the original's shortcut window; the page button turns which row comes first. Rows above the first are labelled with their battle mode keys
- [x] Full screen: F11 (or Settings > Game > Window) switches to borderless full screen at the desktop's resolution and back, remembered between sessions. Alt+Enter is left alone: in the original it sends guild chat
- [x] Screenshots: Print Screen or Scroll Lock saves ScreenShot\screenNNNN.png (lossless PNG at the window's size; the original saved JPEG). Settings > Screenshots chooses whether the interface is included; without it the shot has the world alone (no windows, names, chat or cursor)
- Unverified: the original's strings say Z ~ > is shortcut window 1 (the 3-row era); with four rows this client uses it for the fourth
- [x] Shortcut Settings (MSI_HOTKEYWND, msgstringtable 1486-1494; Esc menu, Shortcut Information and Settings > Controls): the original's Hotkey Bar, Interface and Macros tabs. Click a key, press the new one; Esc removes it, a lone letter is refused ("Unable to specify a single key"), a key already in use asks before moving it, Default restores all. The shortcut bar's labels and Shortcut Information follow the keys. Esc, Enter, Print Screen and the battle mode rows (Settings > Controls) are not in it. Tested with Inventory moved to Ctrl+I (Ctrl+I opens it, Alt+E no longer does); pressing keys in the window itself is covered by unit tests only
- Deliberate difference: Prontera's south avenue paving (`src/mapfix.rs`). The map mirrors its cobble rosettes across the avenue's centre line but sets the mirrored tiles half a rosette lower, so the original shows a seam down the middle; this client aligns them (307 tiles, ground drawing only)

## Packet audit (2026-10-30)
Server packets for PACKETVER 20250402 were taken from rAthena's preprocessed packet
headers and clif.cpp, then compared with what this client reads and sends.
- [x] CZ_REQ_DISCONNECT / ZC_ACK_REQ_DISCONNECT: Exit asks the server; "Please wait 10 seconds before trying to log out." (also for a refused character select)
- [x] 4th-job trait stats (POW..CRT, P.Atk..C.Rate, trait points; CZ_ADVANCED_STATUS_CHANGE) and the AP gauge
- [x] CZ_CHANGE_GROUP_MASTER: delegate party leadership from a member's right-click menu
- [x] CZ_STANDING_RESURRECTION: "Revive" in the death window with a Token of Siegfried or Light of Regene
- [x] CZ_REQ_ONECLICK_ITEMIDENTIFY: identify with a carried Magnifier
- [x] CZ_USE_SKILL_START / END: hold Rolling Cutter's shortcut key
- [x] Inventory items dragged onto the world are dropped (stacks ask how many)
- [x] Selectable item packages (CZ_USE_PACKAGEITEM): using a box listed in selectpackage\selectpackageitem.lub (iRO's, then kRO's newer boxes) opens a window with its sets and their contents; OK asks for that set. Tested with rAthena's Select_Example1 (101060): set 2 gave its four Level 5 weapons. The window shows the client table's contents, the server gives item_packages.yml's, so a custom box needs both
- [x] Super Novice: /doridori (CZ_DORIDORI, the SP regen bonus; our head shakes left and right, others don't see it as the server doesn't tell them) and the guardian angel prayer said line by line in chat (msgstringtable 790-793, CZ_CHOPOKGI). Tested with an Expanded Super Novice: the head shakes; the prayer at 10.03% Base EXP, three separate chat lines, gave Critical Explosion (its cast, CRIT 4 to 54). The whole prayer in one line does nothing, as in the original. No status icon: iRO's stateiconimginfo has none for EFST_EXPLOSIONSPIRITS
- [ ] Pet emotions (CZ_PET_ACT): when the original sends them is unknown
- [x] Costumes (OPTION_WEDDING, XMAS, SUMMER, HANBOK, OKTOBERFEST, SUMMER2: the wedding and rAthena's @costume): the body becomes the costume's sprite (결혼, 산타, 여름, 한복, 옥토버패스트, 여름2) without weapon or shield, as the original does from the option alone. Doram keep their own body for now. /changedress and /nocosplay (CZ_REQ_CHANGEDRESS) take it off: tested with the wedding and Xmas costumes
- [x] Stop attacking with the controller's East button (CZ_CANCEL_LOCKON), /resetskill (CZ_RESET_SKILL, which rAthena runs as @resetskill, so GMs only): both tested in game
- [x] Turning in place (CZ_CHANGE_DIRECTION): Shift+click the ground (or click while sitting; holding the button keeps turning as the mouse goes round), and talking to an NPC turns toward it. One step round turns only the head, a second click the same way brings the body round, wider turns move the body to one step short with the head looking the rest (roBrowserLegacy's lookTo). Others' head turns come from ZC_CHANGE_DIRECTION and the unit packets' headDir; standing and sitting draw the matching head-turn frame
- [x] Ground cursor: texture\grid.tga's corner marks on the walkable cell under the mouse, mint green and half opaque (measured from the original)
- Beyond the original: while aiming a ground skill, the cells it will cover show on the ground in gold (the original marks the one cell). Squares per level from the server's skill_db (crates/ro-client/src/skillarea_db.rs, regenerated with `cargo run -p skillareas -- <rathena-dir>`): unit layout plus the units' range, or the splash area; Fire Wall and Ice Wall turn with the direction cast, Sanctuary, Magnus Exorcismus and Venom Dust keep their shapes (from rAthena's skill.cpp). On a controller it shows at the target, or at our feet
- Beyond the original: a target frame at the top of the screen for the monster we are fighting (the one we attacked or cast on last, else the one hitting us, or the controller's target): its name (gold for bosses, red for MVPs), the monster info line, its HP once the server has sent it, and what it is casting. It lets go 12 s after the last blow, or when the monster dies or leaves; Arrange moves it (Settings > Game > Interface > Monsters > Target frame, on by default)
- Beyond the original: a combat log and damage meter (Alt+D, or More… > Combat Log). The log lists every hit we, our homunculus, mercenary or elemental land and every hit we take (skill, target, damage, hits, criticals, misses) from the attack and skill packets; the meter shows the fight in progress or the last one (damage, damage per second, damage taken; a fight ends after 6 s without hits), the session's totals, and damage by skill with hits, share and biggest hit. Reset clears both
- Beyond the original: while aiming any skill, a ring on the ground around us shows how far it reaches (Settings > Game > Interface > Aiming, on by default). Its size is the server's own test for a player's skills, a circle (rAthena battle_check_range -> check_distance_client: in reach while floor(distance - 0.1) <= range, so ten cells straight out for range 9); it turns red while the target under the cursor is out of reach (casting then walks closer first). The range is the one the server sends in the skill list
- Beyond the original: a monster under the cursor shows its level, element, race and size under its name ("Lv 1 · Water 1 · Plant · Medium"), and Boss or MVP, coloured by element (Settings > Game > Interface > Monsters, on by default). From the server's mob_db with its imports, custom monsters included (crates/ro-client/src/mobinfo_db.rs, regenerated with `cargo run -p mobinfo -- <rathena-dir>`); the level is the one the server sends
- [x] Storage: Shift+right-click takes an item into the inventory's favourites tab (CZ_MOVE_ITEM_TO_PERSONAL, clients from 2025-01-22). Tested: the stack shows in the favourites tab at once (ZC_ITEM_PICKUP_ACK's favourite flag, which was read as off); a stackable item already carried joins its stack where it is, as the server does
- [ ] Private airship (CZ_PRIVATE_AIRSHIP_REQUEST): needs a map picker; nothing on the server opens it
- [x] GM tools: /b /nb /lb /nlb /hide /mm /recall /shift /item /monster /rc /check /killall /resetstate /resetcooltime /changemaptype, and GM right-click actions (config `gm`)
- [ ] GM macro-detector captcha registration and preview (CZ_UPLOAD_MACRO_DETECTOR_CAPTCHA, CZ_REQ_PREVIEW_MACRO_DETECTOR)
- Not planned: auction house, pre-RODEX mail and the old party booking (replaced at this client version); account-name lookup (rAthena always answers with an empty name)
