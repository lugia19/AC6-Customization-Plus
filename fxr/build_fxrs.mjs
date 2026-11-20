// Builds the custom generator flames. The game has a flame per effect row and generator type (combustion, current, Coral):
// the given rows, three stock thruster FXRs each. Only the player's AC gets the custom types (the DLL hooks the
// generator type, dll/src/columns.rs). For every stock FXR S in them:
// - a bundle at S + BUNDLE_OFFSET (what the custom type's column points at), which plays the two copies of S together;
// - the copies at S + MAIN.offsets (add, remove): plain copies of S (and of every FXR S references, remapped to
//   the same copy), every color whitened to its brightness and tinted by external values the DLL serves
//   for the player's thruster color (dll/src/ext_values.rs set_channels):
//   - add: the flame with the color added (lights and lens flares too); alpha-blended particles (smoke) get the plain
//     color, which they can show as is;
//   - remove: the additive particles again, subtracting what the color lacks from the background, so dark colors
//     darken it (additive flames can't get darker than what's behind them). Nothing for bright colors.
// A fourth flame type, ALLMIND, plays the boss's own effects on some rows (ALLMIND), every flame fading from the accent
// color to the thruster color (two more pairs of copies; the accent is a second set of external values), in a bundle at
// the row's current flame + ALLMIND_BUNDLE_OFFSET.
// Other effects (the core expansions') get the same copies, tinted by the expansion color instead, in a bundle at
// S + BUNDLE_OFFSET, for every flame type. Melee weapons' effects get one copy each at S + WEAPON_OFFSET, tinted by
// their weapon's color.
// Usage: node build_fxrs.mjs <unpacked sfxbnd_commoneffects effect dir> <id map json out> <input json>
//        <unpacked sfxbnd_m30 effect dir> [bundles dir]
// (input: {flames: a list of [combustion, current, Coral] stock FXR IDs, others: a list of stock FXR IDs,
// weapons: a list of [first external value ID, stock FXR IDs], neutralModels: {model: the copies' version of it, whose
// material has no color of its own}})
// Writes the new FXRs into the effect dir, and a map {bundles: per flame row, its bundle per flame type (combustion,
// current, Coral, ALLMIND), otherBundles: {stock id: the bundle or copy its custom columns play}, resources: {new id: source id}} (each new FXR needs its
// source's resource list; bundles only hold proxies).
// With a bundles dir, only the bundles and the copies of the boss's effects are built, into it, overwriting, and no
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

// The colors the DLL serves (dll/src/ext_values.rs), each as nine external values from `ext`: what the add copy adds,
// what the remove copy subtracts, the plain color for alpha-blended particles; red, green, blue each, as 1 + a 0..1
// amount; 0 means no DLL. Each kind of copy has its own [add, remove] ID offsets.
const MAIN = { ext: 900001, offsets: [9100000, 9200000] } // the thruster color
// The core expansions' effects: the expansion color. Their effects are no flame's, so the same offsets.
// Greyed by luminance (see recolor), so white is about as bright as the stock effect.
const EXPANSION = { ext: 900021, offsets: [9100000, 9200000], byLuminance: true }
// The ALLMIND type's flames fade from the accent color to the thruster color: each is played twice, an accent copy and
// a thruster color copy, weighted so that the two (additive) add up to the fade.
const FADING_ACCENT = { ext: 900012, offsets: [9300000, 9400000], accent: true }
const FADING_MAIN = { ext: 900001, offsets: [9500000, 9600000], accent: false }
// Melee weapons' effects (build.py WEAPON_EFFECTS): one copy each (no subtractive one), tinted by the weapon's own
// color, which their replace rows play directly (listed in otherBundles).
const WEAPON_OFFSET = 9700000
// Game value: the spawner's termination signal.
const EXT_TERMINATE = 0
// Bundle IDs: a row's flame for a stock type + BUNDLE_OFFSET; its current flame + ALLMIND_BUNDLE_OFFSET for ALLMIND.
const BUNDLE_OFFSET = 9000000
const ALLMIND_BUNDLE_OFFSET = 8000000

// How a flame fades: BY_AGE, each particle from the accent at birth to the thruster color at FADE_TIME (seconds);
// BY_HUE, where the boss's own effect fades from mint to cyan (weighted by how far each color is from mint (1)
// towards cyan (0)).
const BY_AGE = 'age'
const BY_HUE = 'hue'
const FADE_TIME = 0.15
const MINT_HUE = 160
const CYAN_HUE = 190

// ALLMIND (a boss, not an AC) has its own thruster effects, in sfxbnd_m30. The ALLMIND flame type plays, per flame row
// (its combustion flame's ID, else its first two digits), these effects, faded so ('current' = the row's current
// flame); rows not listed play their current flame.
const ALLMIND = {
  41: [['current', BY_AGE], [642001, BY_AGE]], // regular boosting: plus the boss's spark spray, steady light, start flash
  45: [[642002, BY_AGE]], // quick boost: the boss's
  440011: [[642142, BY_HUE]], // assault boost, held: the boss's burst from a special move, made continuous
}
// Made for a boss several times an AC's size: what each of its effects needs on an AC (tuned in game).
const ALLMIND_CHANGES = {
  642001: { withoutFlames: true }, // its flames are the AC's own current flame
  642002: { scale: 0.4 },
  642005: { scale: 0.4 }, // 642002's nozzle flames
  642142: { scale: 0.3, continuous: true },
}
const CURRENT = 1

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

// The accent's share at a particle age: 1 at birth, 0 from FADE_TIME on (`accent`), or the rest.
function ageShare(age, accent) {
  const t = Math.min(1, age / FADE_TIME)
  return accent ? 1 - t : t
}

// A color property over particle age, multiplied by the accent's share: resampled as a linear curve at its own
// keyframes, the fade's and a few in between, keeping its modifiers. Null for a looping one.
function ageFaded(property, accent) {
  if (Array.isArray(property)) property = new ConstantProperty(property)
  if (property.loop) return null
  const positions = new Set([0, FADE_TIME / 2, FADE_TIME])
  for (const keyframes of [property.keyframes ?? [], ...(property.components ?? [])]) {
    for (const { position } of keyframes) positions.add(position)
  }
  const end = Math.max(...positions)
  for (let i = 1; i < 8; i++) positions.add((end * i) / 8)
  const faded = new LinearProperty(false, [...positions].sort((a, b) => a - b).map(age => {
    const [r, g, b, a] = property.valueAt(age)
    const share = ageShare(age, accent)
    return new Keyframe(age, [r * share, g * share, b * share, a])
  }))
  faded.modifiers = [...(property.modifiers ?? [])]
  return faded
}

// One copy of a fading effect (`accent`: the accent copy, else the thruster color one), each particle's brightness
// scaled by the accent's share or the rest:
// - BY_AGE: over the particle's age, on color1 (particle age on every particle kind);
// - BY_HUE: by how mint each color is, on the particle's one color property that changes hue over time (the boss's
//   fade), else its first colored one.
// Anything else (no usable color, lights, lens flares by age) gets half in each copy. Distortion is only in the
// thruster color copy.
function weighFade(fxr, accent, by) {
  const half = ([r, g, b, a]) => [r / 2, g / 2, b / 2, a]
  const byHue = color => {
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
      Object.entries(action.$data.props).filter(([key, meta]) => 'color' in meta && !('field' in meta))
        .map(([key]) => ({ action, key, weights: colorsOf(action, key).map(mintWeight).filter(w => w !== null) })))
    if (!appearance.$data.isParticle) { // lights, lens flares: each color their own
      for (const { action, key } of colored) action.recolorProperty(key, by === BY_HUE ? byHue : half)
      continue
    }
    if (by === BY_AGE && 'color1' in appearance.$data.props) {
      const faded = ageFaded(appearance.color1, accent)
      if (faded) {
        appearance.color1 = faded
        continue
      }
    }
    const withHue = colored.filter(({ weights }) => weights.length)
    const chosen = by === BY_HUE &&
      (withHue.find(({ weights }) => Math.max(...weights) - Math.min(...weights) > 0.1) ?? withHue[0])
    if (chosen) {
      chosen.action.recolorProperty(chosen.key, byHue)
    } else {
      const [target, key] = tintTarget(config)
      target.recolorProperty(key, half)
    }
  }
}

// Recolors a copy for `color`. Everything is whitened; the tint goes on each particle's tint target. Lights and lens
// flares get the tint on their own colors. Distortion stays as it is: tinting it darkens the background into dark shapes.
// `remove`: additive particles subtract from the background instead, without bloom. Everything the add copy already
// draws is hidden: alpha-blended particles (subtracting them would darken their whole quad), lights, lens flares and
// distortion.
const luminance = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b

// The colors a particle multiplies together (its appearance's color1-3 and its modifier's color), at age 0.
function colorProduct(config) {
  const at = property => (Array.isArray(property) ? property : property?.valueAt?.(0))
  const keys = ['color1', 'color2', 'color3'].filter(key => key in config.appearance.$data.props)
  const factors = [...keys.map(key => at(config.appearance[key])), at(config.particleModifier?.color)].filter(Boolean)
  return [0, 1, 2].map(channel => factors.reduce((product, factor) => product * factor[channel], 1))
}

function recolor(fxr, remove, color) {
  const [extAdd, extRemove, extPlain] = [0, 3, 6].map(offset => color.ext + offset)
  // byLuminance: each particle's greyed colors are then scaled so that their product has the stock product's luminance
  // (the product of greyed factors isn't the grey of their product).
  const particles = [...fxr.root.walkConfigs()].filter(config =>
    config.appearance?.$data?.isParticle && config.appearance.type !== ActionType.Distortion)
  const stockLuminance = new Map(color.byLuminance ? particles.map(config => [config, luminance(colorProduct(config))]) : [])
  // Every color property: whitened (byLuminance: greyed to its luminance), and tinted for lights (hidden in the remove
  // copy).
  const grey = color.byLuminance ? ([r, g, b, a]) => [...Array(3).fill(luminance([r, g, b])), a] : whiten
  for (const action of fxr.root.walkActions()) {
    if (!action.$data || !('props' in action.$data) || action.type === ActionType.Distortion) continue
    for (const [key, meta] of Object.entries(action.$data.props)) {
      if (!('color' in meta)) continue
      if (!isLightColor(action, key)) {
        action.recolorProperty(key, grey)
      } else if (remove) {
        action.recolorProperty(key, () => [0, 0, 0, 0])
      } else {
        action.recolorProperty(key, grey)
        addTint(action, key, extAdd)
      }
    }
  }
  for (const [config, target] of stockLuminance) {
    const whitened = colorProduct(config)[0]
    if (!whitened) continue
    const [action, key] = tintTarget(config)
    action.recolorProperty(key, ([r, g, b, a]) => [r * target / whitened, g * target / whitened, b * target / whitened, a])
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

// The ALLMIND type's effects for a row: both fading copies of each, as [effect, copy kind, how it fades].
function allmindEffects(rowFlames) {
  const entries = ALLMIND[rowFlames[0]] ?? ALLMIND[Math.floor(rowFlames[0] / 10000)] ?? [['current', BY_AGE]]
  return entries.flatMap(([effect, by]) => {
    const sfx = effect === 'current' ? rowFlames[CURRENT] : effect
    return [FADING_ACCENT, FADING_MAIN].map(kind => [sfx, kind, by])
  })
}

// The effects of each flame type for a row, as [effect, copy kind, how it fades]: the row's flames, then ALLMIND.
const flameTypes = rowFlames => [...rowFlames.map(flame => [[flame, MAIN, null]]), allmindEffects(rowFlames)]

// The bundle a row's flame type plays (what its custom column points at): every copy of every effect of the type.
function bundle(id, effects) {
  const proxies = effects.flatMap(([sfx, kind]) => kind.offsets.map(offset => new ProxyNode(sfx + offset)))
  return new FXR(id, [State.from([`External(${EXT_TERMINATE}) >= 1 then terminate`])], [new BasicNode([], proxies)])
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

const [effectDir, idMapPath, inputPath, allmindDir, bundlesDir] = process.argv.slice(2)
const { flames: rows, others, weapons, neutralModels } = JSON.parse(fs.readFileSync(inputPath, 'utf8'))
const fileName = id => `f${String(id).padStart(9, '0')}.fxr`
const file = id => path.join(effectDir, fileName(id))
// The boss's effects are in their own archive.
const isAllmind = id => !fs.existsSync(file(id))
const sourceFile = id => (isAllmind(id) ? path.join(allmindDir, fileName(id)) : file(id))
// A path, not fs.readFileSync: small files come back as views into Node's shared buffer pool, which the library
// reads from the pool's start.
const read = id => FXR.read(sourceFile(id), Game.ArmoredCore6)
// The effect dir is a fresh unpack of the game's effects, so an existing file at a new ID is a stock effect. The
// bundles dir is ours to overwrite.
const saved = new Set()
async function save(fxr) {
  if (saved.has(fxr.id)) throw new Error(`${fxr.id} built twice`)
  saved.add(fxr.id)
  if (bundlesDir) return fxr.saveAs(path.join(bundlesDir, fileName(fxr.id)), Game.ArmoredCore6)
  if (fs.existsSync(file(fxr.id))) throw new Error(`${fxr.id} would overwrite an existing effect`)
  await fxr.saveAs(file(fxr.id), Game.ArmoredCore6)
}

// Every effect a flame type plays, as its kind of copy, and everything it references through proxies (same kind): one
// job each, keyed by effect and kind, with how it fades.
const jobs = new Map()
const pending = [
  ...rows.flatMap(rowFlames => flameTypes(rowFlames).flat()),
  ...others.map(sfx => [sfx, EXPANSION, null]),
  ...weapons.flatMap(([ext, effects]) => {
    const kind = { ext, offsets: [WEAPON_OFFSET] }
    return effects.map(sfx => [sfx, kind, null])
  }),
]
while (pending.length) {
  const [sfx, color, fade] = pending.pop()
  const key = `${sfx}:${color.offsets[0]}`
  if (jobs.has(key) && jobs.get(key)[1] !== color) throw new Error(`${sfx} is wanted in two colors at the same IDs`)
  if (jobs.has(key)) {
    if (jobs.get(key)[2] !== fade) throw new Error(`${sfx} is wanted faded two ways`)
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
  if (bundlesDir && !isAllmind(sfx)) continue
  for (const [index, offset] of color.offsets.entries()) {
    const fxr = await read(sfx)
    const changes = ALLMIND_CHANGES[sfx] ?? {}
    if (changes.scale) fxr.root.scale(changes.scale)
    if (changes.continuous) makeContinuous(fxr)
    for (const node of fxr.root.walk()) {
      if (changes.withoutFlames && node.nodes) node.nodes = node.nodes.filter(child => !(child instanceof ProxyNode))
      if (node instanceof ProxyNode) node.sfx += offset
    }
    for (const config of fxr.root.walkConfigs()) {
      const model = config.appearance?.model?.value ?? config.appearance?.model
      if (model in neutralModels) config.appearance.model = neutralModels[model]
    }
    if (fade) weighFade(fxr, color.accent, fade)
    recolor(fxr, index === 1, color)
    fxr.id = sfx + offset
    resources[fxr.id] = sfx
    await save(fxr)
  }
}

// Each row's bundles, one per flame type. A stock flame is one row's (build.py checks), so a row with the same flame
// for several types gets one bundle.
const bundles = []
for (const rowFlames of rows) {
  const ids = [...rowFlames.map(flame => flame + BUNDLE_OFFSET), rowFlames[CURRENT] + ALLMIND_BUNDLE_OFFSET]
  for (const [type, effects] of flameTypes(rowFlames).entries()) {
    if (!saved.has(ids[type])) await save(bundle(ids[type], effects))
  }
  bundles.push(ids)
}
const otherBundles = {}
for (const sfx of others) {
  otherBundles[sfx] = sfx + BUNDLE_OFFSET
  await save(bundle(sfx + BUNDLE_OFFSET, [[sfx, EXPANSION]]))
}
for (const [, effects] of weapons) {
  for (const sfx of effects) otherBundles[sfx] = sfx + WEAPON_OFFSET
}
if (bundlesDir) {
  console.log(`${saved.size - Object.keys(resources).length} bundles and the boss's effects in ${bundlesDir}`)
} else {
  fs.writeFileSync(idMapPath, JSON.stringify({ bundles, otherBundles, resources }, null, 1))
  console.log(`${saved.size - Object.keys(resources).length} bundles, ${Object.keys(resources).length} copies (${jobs.size} effect colors)`)
}
