// Builds the custom generator flames. The game has a flame per effect row and generator type (combustion, current, Coral):
// the given rows, three stock thruster FXRs each. For every stock FXR S in them:
// - a wrapper at S + WRAPPER_OFFSET (what the params point at), which plays either the stock S or, on the player's AC,
//   two custom copies together: of S, or of its row's flame for the flame type the player chose (see wrapper());
// - the copies at S + MAIN.offsets (add, remove): plain copies of S (and of every FXR S references, remapped to
//   the same copy), every color whitened to its brightness and tinted by external values the DLL serves
//   for the player's thruster color (dll/src/thrusters.rs set_channels):
//   - add: the flame with the color added (lights and lens flares too); alpha-blended particles (smoke) get the plain
//     color, which they can show as is;
//   - remove: the additive particles again, subtracting what the color lacks from the background, so dark colors
//     darken it (additive flames can't get darker than what's behind them). Nothing for bright colors.
// A fourth flame type, ALLMIND, plays the boss's own effects on some rows (ALLMIND), in the thruster color or the
// accent color (a second set of external values and copies).
// Usage: node build_fxrs.mjs <unpacked sfxbnd_commoneffects effect dir> <id map json out> <rows json>
//        <unpacked sfxbnd_m30 effect dir> [wrappers dir]
// (rows: a list of [combustion, current, Coral] stock FXR IDs)
// Writes the new FXRs into the effect dir, and a map {wrappers: {stock id: wrapper id}, resources: {new id: source id},
// flameTypes: how many types a wrapper offers} (each new FXR needs its source's resource list; wrappers only hold
// proxies).
// With a wrappers dir, only the wrappers and the copies of the boss's effects are built, into it, overwriting, and no
// map: for trying them in the running game with an FXR reloader.
import {
  ActionType,
  BasicNode,
  BlendMode,
  ConstantProperty,
  ExternalValue1Modifier,
  FXR,
  Game,
  Keyframe,
  LinearProperty,
  ParticleModifier,
  PeriodicEmitter,
  ProxyNode,
  State,
} from '@cccode/fxr'
import fs from 'node:fs'
import path from 'node:path'

// The colors the DLL serves (dll/src/thrusters.rs), each as nine external values from `ext`: what the add copy adds,
// what the remove copy subtracts, the plain color for alpha-blended particles; red, green, blue each, as 1 + a 0..1
// amount; 0 means no DLL. Each color has its own pair of copies of an effect: [add, remove] ID offsets.
const MAIN = { ext: 900001, offsets: [9100000, 9200000] } // the thruster color
const ACCENT = { ext: 900012, offsets: [9300000, 9400000] }
// Whether the flame is on the player's AC (dll/src/player_flames.rs): 0 not known yet (or no DLL), 1 someone else's,
// 2 the player's.
const EXT_PLAYERS_FLAME = 900010
// The flame type the player chose (dll/src/thrusters.rs): 0 the generator's own, 1 + a type (0 combustion, 1 current,
// 2 Coral, 3 ALLMIND) otherwise.
const EXT_FLAME_TYPE = 900011
// Game value: the spawner's termination signal.
const EXT_TERMINATE = 0
const WRAPPER_OFFSET = 9000000

// ALLMIND (a boss, not an AC) has its own thruster effects, in sfxbnd_m30. The ALLMIND flame type plays, per flame row
// (its combustion flame's ID, else its first two digits), these effects in these colors ('current' = the row's current
// flame; FADE = mint fading to cyan in the boss, the accent fading to the thruster color here); rows not listed play
// their current flame.
const FADE = 'fade'
const ALLMIND = {
  41: [['current', MAIN], [642001, MAIN]], // regular boosting: plus the boss's spark spray, steady light, start flash
  45: [[642002, MAIN]], // quick boost: the boss's
  440001: [['current', ACCENT]], // assault boost start
  440011: [[642142, FADE]], // assault boost, held: the boss's burst from a special move, made continuous
  440101: [['current', ACCENT]], // assault boost end
}
// Made for a boss several times an AC's size: what each of its effects needs on an AC (tuned in game).
const ALLMIND_CHANGES = {
  642001: { withoutFlames: true }, // its flames are the AC's own current flame
  642002: { scale: 0.4 },
  642005: { scale: 0.4 }, // 642002's nozzle flames
  642142: { scale: 0.3, continuous: true },
}
const CURRENT = 1

// FADE effects are played twice: an accent copy and a thruster color copy, weighted by how far each color is from
// the boss's mint (1) towards its cyan (0), by hue. Additive particles add up to the fade.
const MINT_HUE = 160
const CYAN_HUE = 190

// Blend modes that add light: Add, and Unk0 / Unk7 (the library documents both as behaving like Add).
const ADDITIVE = new Set([BlendMode.Add, BlendMode.Unk0, BlendMode.Unk7])

// Keeps brightness and alpha, drops the hue: the tint supplies it.
const whiten = ([r, g, b, a]) => {
  const m = Math.max(r, g, b)
  return [m, m, m, a]
}

// A color multiplier that scales only one channel (0 red, 1 green, 2 blue) by `amount`.
function scaleChannel(channel, amount) {
  const multiplier = [1, 1, 1, 1]
  multiplier[channel] = amount
  return multiplier
}

// Multiplies a color property's red, green and blue by the amounts the DLL serves at `ext`, `ext + 1` and `ext + 2`.
// The DLL serves 1 + amount, so the curve maps 1 -> 0 and 2 -> 1; 0 (no DLL) maps to `withoutDll`.
function addTint(action, key, ext, withoutDll = 1) {
  if ('field' in action.$data.props[key]) return // fields can't carry modifiers
  if (Array.isArray(action[key])) {
    action[key] = new ConstantProperty(action[key])
  }
  for (const channel of [0, 1, 2]) {
    const curve = new LinearProperty(false, [
      new Keyframe(0, scaleChannel(channel, withoutDll)),
      new Keyframe(1, scaleChannel(channel, 0)),
      new Keyframe(2, scaleChannel(channel, 1)),
    ])
    action[key].modifiers.push(new ExternalValue1Modifier(ext + channel, curve))
  }
}

// Lights and lens flare layers: they can only add light, and get the tint on their own colors.
function isLightColor(action, key) {
  if (action.type === ActionType.PointLight) return true
  return action.type === ActionType.LensFlare && /^layer\dColor$/.test(key)
}

// An appearance's blend mode is either a number or a property holding one.
function blendModeOf(appearance) {
  const mode = appearance.blendMode
  return typeof mode === 'number' ? mode : mode?.value
}

// A particle's color is the product of several properties (the appearance's colors, the config's ParticleModifier
// color...), so the tint goes on exactly one factor: the ParticleModifier, or color2 for Model particles, which ignore
// the ParticleModifier's color (found with fxr-hunter).
function tintTarget(config) {
  if (config.appearance.type === ActionType.Model) return [config.appearance, 'color2']
  config.particleModifier ??= new ParticleModifier()
  return [config.particleModifier, 'color']
}

// How mint a color is (1) rather than cyan (0); null for a grey one.
function mintWeight([r, g, b]) {
  const max = Math.max(r, g, b), range = max - Math.min(r, g, b)
  if (range < 0.05 * max || max === 0) return null
  const sector = max === r ? (g - b) / range : max === g ? 2 + (b - r) / range : 4 + (r - g) / range
  const hue = ((sector * 60) % 360 + 360) % 360
  return Math.min(1, Math.max(0, (CYAN_HUE - hue) / (CYAN_HUE - MINT_HUE)))
}

// Every color property's values, keyframes included.
function colorsOf(action, key) {
  const colors = []
  action.recolorProperty(key, color => (colors.push(color), color))
  return colors
}

// One copy of a FADE effect: every particle's (and light's) brightness scaled by its mint weight (`accent`) or the
// rest. Per particle, the weight comes from the one color property that changes hue over time if there is one (the
// fade), else from its first colored one; a particle with no color at all gets half in each copy. Distortion is
// only in the thruster color copy.
function weighFade(fxr, accent) {
  const weigh = color => {
    const w = mintWeight(color)
    return w === null ? color : [...color.slice(0, 3).map(c => c * (accent ? w : 1 - w)), color[3]]
  }
  for (const config of fxr.root.walkConfigs()) {
    const appearance = config.appearance
    if (!appearance?.$data?.props || appearance.type === ActionType.None) continue
    if (appearance.type === ActionType.Distortion) {
      if (accent) for (const key of ['sizeX', 'sizeY', 'sizeZ']) if (key in appearance.$data.props) appearance[key] = 0
      continue
    }
    const colored = [appearance, config.particleModifier].filter(Boolean).flatMap(action =>
      Object.entries(action.$data.props).filter(([key, meta]) => 'color' in meta).map(([key]) => [action, key])
        .map(([action, key]) => ({ action, key, weights: colorsOf(action, key).map(mintWeight).filter(w => w !== null) }))
        .filter(({ weights }) => weights.length))
    if (!appearance.$data.isParticle) { // lights, lens flares: each color their own
      for (const { action, key } of colored) action.recolorProperty(key, weigh)
      continue
    }
    const chosen = colored.find(({ weights }) => Math.max(...weights) - Math.min(...weights) > 0.1) ?? colored[0]
    if (chosen) {
      chosen.action.recolorProperty(chosen.key, weigh)
    } else {
      const [target, key] = tintTarget(config)
      target.recolorProperty(key, ([r, g, b, a]) => [r / 2, g / 2, b / 2, a])
    }
  }
}

// Recolors a copy for `color`. Everything is whitened; the tint goes on each particle's tint target. Lights and lens
// flares get the tint on their own colors. Distortion stays as it is: tinting it darkens the background into dark shapes.
// `remove`: additive particles subtract from the background instead, without bloom. Everything the add copy already
// draws is hidden: alpha-blended particles (subtracting them would darken their whole quad), lights, lens flares and
// distortion.
function recolor(fxr, remove, color) {
  const [extAdd, extRemove, extPlain] = [0, 3, 6].map(offset => color.ext + offset)
  // Every color property: whitened, and tinted for lights (hidden in the remove copy).
  for (const action of fxr.root.walkActions()) {
    if (!action.$data || !('props' in action.$data) || action.type === ActionType.Distortion) continue
    for (const [key, meta] of Object.entries(action.$data.props)) {
      if (!('color' in meta)) continue
      if (!isLightColor(action, key)) {
        action.recolorProperty(key, whiten)
      } else if (remove) {
        action.recolorProperty(key, () => [0, 0, 0, 0])
      } else {
        action.recolorProperty(key, whiten)
        addTint(action, key, extAdd)
      }
    }
  }
  for (const config of fxr.root.walkConfigs()) {
    const appearance = config.appearance
    if (!appearance?.$data?.isParticle) continue
    if (appearance.type === ActionType.Distortion) {
      if (remove) {
        for (const key of ['sizeX', 'sizeY', 'sizeZ']) {
          if (key in appearance.$data.props) appearance[key] = 0
        }
      }
      continue
    }
    const [target, key] = tintTarget(config)
    const additive = ADDITIVE.has(blendModeOf(appearance))
    if (!remove) {
      addTint(target, key, additive ? extAdd : extPlain)
    } else if (additive) {
      appearance.blendMode = BlendMode.Subtract
      if ('bloom' in appearance.$data.props) appearance.bloom = false
      addTint(target, key, extRemove, 0)
    } else {
      target[key] = [1, 1, 1, 0]
    }
  }
}

// The ALLMIND type's effects for a row: [effect, color, fade], a FADE effect as both its copies.
function allmindEffects(rowFlames) {
  const entries = ALLMIND[rowFlames[0]] ?? ALLMIND[Math.floor(rowFlames[0] / 10000)] ?? [['current', MAIN]]
  return entries.flatMap(([effect, color]) => {
    const sfx = effect === 'current' ? rowFlames[CURRENT] : effect
    return color === FADE ? [[sfx, ACCENT, true], [sfx, MAIN, true]] : [[sfx, color, false]]
  })
}

// The effects of each flame type for a row, as [effect, color, fade]: the row's flames, then ALLMIND.
const flameTypes = rowFlames => [...rowFlames.map(flame => [[flame, MAIN, false]]), allmindEffects(rowFlames)]

// The wrapper for stock flame `stockId`, which is its row's flame for generator type `ownType`: a child node per effect
// it can play (a proxy to it). STOCK enables the stock flame; CUSTOM + a type enables both copies of each effect of
// that type. The choice is made once, at spawn, and kept: a proxy's effect keeps playing after its node is
// disabled (switching from stock to custom left the stock flame running under the custom one), so a wrapper may only
// ever enable one of them. Conditions are checked in order, first match wins:
// - DECIDING (shows nothing): no DLL (its color values read 0) -> STOCK; the player's flame -> PICKING; someone else's
//   -> STOCK; still not known after 0.3 s -> STOCK (the DLL answers on the owner's next slot update, a frame after
//   the spawn; the wait only lasts while there's no answer, e.g. a flame a cutscene spawned).
// - PICKING (shows nothing): the chosen flame type's CUSTOM state; the generator's own type if none is chosen.
// - STOCK, CUSTOM + type: only end with the effect.
function wrapper(stockId, rowFlames, ownType) {
  const playersFlame = `External(${EXT_PLAYERS_FLAME})`
  const flameType = `External(${EXT_FLAME_TYPE})`
  const terminate = `External(${EXT_TERMINATE}) >= 1 then terminate`
  const STOCK = 1, PICKING = 2, CUSTOM = 3 // DECIDING is 0, where an effect starts
  const types = flameTypes(rowFlames)
  // The served value is 1 + the type: highest first, since the first match wins.
  const pick = types.map((_, type) => `${flameType} >= ${type + 1} then ${CUSTOM + type}`).reverse()
  const states = [
    // DECIDING
    State.from([
      terminate,
      `External(${MAIN.ext}) < 1 then ${STOCK}`,
      `${playersFlame} >= 2 then ${PICKING}`,
      `${playersFlame} >= 1 then ${STOCK}`,
      `StateTime > 0.3 then ${STOCK}`,
    ]),
    // STOCK
    State.from([terminate]),
    // PICKING
    State.from([terminate, ...pick, `${flameType} < 1 then ${CUSTOM + ownType}`]),
    // CUSTOM...
    ...pick.map(() => State.from([terminate])),
  ]
  // A node with `actions` over `nodes`, enabled (config 0) only in `state`, disabled (-1) in the others.
  const inState = (state, actions, nodes) => {
    const configPerState = states.map((_, s) => (s === state ? 0 : -1))
    return new BasicNode(actions, nodes).mapStates(...configPerState)
  }
  const child = (sfx, state) => inState(state, [], [new ProxyNode(sfx)])
  return new FXR(stockId + WRAPPER_OFFSET, states, [
    child(stockId, STOCK),
    ...types.flatMap((effects, type) =>
      effects.flatMap(([sfx, color]) => color.offsets.map(offset => child(sfx + offset, CUSTOM + type)))),
  ])
}

// Ends with its spawner; every repeating emitter and every plain container runs forever. One-time particles (a start
// light, distortion) stay a start flash.
function makeContinuous(fxr) {
  const FOREVER = -1 / 30
  const plain = v => v?.value ?? v
  fxr.states = [State.from([`External(${EXT_TERMINATE}) >= 1 then terminate`])]
  for (const config of fxr.root.walkConfigs()) {
    if (!config.nodeAttributes || plain(config.nodeAttributes.duration) < 0) continue
    if (!config.appearance || config.appearance.type === ActionType.None) {
      config.nodeAttributes.duration = FOREVER
      if (config.particleAttributes) config.particleAttributes.duration = FOREVER
    } else if (config.emitter instanceof PeriodicEmitter) {
      config.nodeAttributes.duration = FOREVER
    }
  }
}

const [effectDir, idMapPath, rowsPath, allmindDir, wrappersDir] = process.argv.slice(2)
const rows = JSON.parse(fs.readFileSync(rowsPath, 'utf8'))
const stockIds = [...new Set(rows.flat())]
const fileName = id => `f${String(id).padStart(9, '0')}.fxr`
const file = id => path.join(effectDir, fileName(id))
// The boss's effects are in their own archive.
const isAllmind = id => !fs.existsSync(file(id))
const sourceFile = id => (isAllmind(id) ? path.join(allmindDir, fileName(id)) : file(id))
// A path, not fs.readFileSync: small files come back as views into Node's shared buffer pool, which the library
// reads from the pool's start.
const read = id => FXR.read(sourceFile(id), Game.ArmoredCore6)
// The effect dir is a fresh unpack of the game's effects, so an existing file at a new ID is a stock effect. The
// wrappers dir is ours to overwrite.
const saved = new Set()
async function save(fxr) {
  if (saved.has(fxr.id)) throw new Error(`${fxr.id} built twice`)
  saved.add(fxr.id)
  if (wrappersDir) return fxr.saveAs(path.join(wrappersDir, fileName(fxr.id)), Game.ArmoredCore6)
  if (fs.existsSync(file(fxr.id))) throw new Error(`${fxr.id} would overwrite an existing effect`)
  await fxr.saveAs(file(fxr.id), Game.ArmoredCore6)
}

// Every effect a flame type plays, in its color, and everything it references through proxies (same color): one job
// each, keyed by effect and color, with whether it's a FADE copy.
const jobs = new Map()
const pending = rows.flatMap(rowFlames => flameTypes(rowFlames).flat())
while (pending.length) {
  const [sfx, color, fade] = pending.pop()
  const key = `${sfx}:${color.ext}`
  if (jobs.has(key)) {
    if (jobs.get(key)[2] !== fade) throw new Error(`${sfx} is wanted both faded and not`)
    continue
  }
  jobs.set(key, [sfx, color, fade])
  if (ALLMIND_CHANGES[sfx]?.withoutFlames) continue
  for (const node of (await read(sfx)).root.walk()) {
    if (node instanceof ProxyNode) pending.push([node.sfx, color, fade])
  }
}

// Both copies of each: the boss's effects changed for an AC first, then faded, recolored, proxies remapped.
const resources = {}
for (const [sfx, color, fade] of jobs.values()) {
  if (wrappersDir && !isAllmind(sfx)) continue
  for (const [index, offset] of color.offsets.entries()) {
    const fxr = await read(sfx)
    const changes = ALLMIND_CHANGES[sfx] ?? {}
    if (changes.scale) fxr.root.scale(changes.scale)
    if (changes.continuous) makeContinuous(fxr)
    for (const node of fxr.root.walk()) {
      if (changes.withoutFlames && node.nodes) node.nodes = node.nodes.filter(child => !(child instanceof ProxyNode))
      if (node instanceof ProxyNode) node.sfx += offset
    }
    if (fade) weighFade(fxr, color === ACCENT)
    recolor(fxr, index === 1, color)
    fxr.id = sfx + offset
    resources[fxr.id] = sfx
    await save(fxr)
  }
}

// A stock flame is one row's, for one type; a row with the same flame for every type gets one wrapper.
const wrappers = {}
for (const rowFlames of rows) {
  for (const [type, stockId] of rowFlames.entries()) {
    if (stockId in wrappers) continue
    const fxr = wrapper(stockId, rowFlames, type)
    await save(fxr)
    wrappers[stockId] = fxr.id
  }
}
if (wrappersDir) {
  console.log(`${Object.keys(wrappers).length} wrappers and the boss's effects in ${wrappersDir}`)
} else {
  fs.writeFileSync(idMapPath, JSON.stringify({ wrappers, resources, flameTypes: rows[0].length + 1 }, null, 1))
  console.log(`${stockIds.length} wrappers, ${Object.keys(resources).length} copies (${jobs.size} effect colors)`)
}
