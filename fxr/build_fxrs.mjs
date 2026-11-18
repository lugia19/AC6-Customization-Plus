// Builds the custom generator flames. The game has a flame per effect row and generator type (combustion, current, Coral):
// the given rows, three stock thruster FXRs each. For every stock FXR S in them:
// - a wrapper at S + WRAPPER_OFFSET (what the params point at), which plays either the stock S or, on the player's AC,
//   two custom copies together: of S, or of its row's flame for the flame type the player chose (see wrapper());
//   three more types, the "ALLMIND fanout" of each, are the row's flame for a type several times over, fanned out;
// - the copies at S + ADD_OFFSET and S + REMOVE_OFFSET: plain copies of S (and of every FXR S references, remapped to
//   the same copy), every color whitened to its brightness and tinted by external values the DLL serves
//   for the player's thruster color (dll/src/thrusters.rs set_channels):
//   - add: the flame with the color added (lights and lens flares too); alpha-blended particles (smoke) get the plain
//     color, which they can show as is;
//   - remove: the additive particles again, subtracting what the color lacks from the background, so dark colors
//     darken it (additive flames can't get darker than what's behind them). Nothing for bright colors.
// Usage: node build_fxrs.mjs <unpacked sfxbnd_commoneffects effect dir> <id map json out> <rows json>
//   <unpacked sfxbnd_m30 effect dir> [wrappers dir]
// (rows: a list of [combustion, current, Coral] stock FXR IDs; sfxbnd_m30 has ALLMIND's thruster effects)
// Writes the new FXRs into the effect dir, and a map {wrappers: {stock id: wrapper id}, resources: {new id: source id},
// allmindResources: the same for sources in sfxbnd_m30, flameTypes: how many types a wrapper offers} (each new FXR
// needs its source's resource list; wrappers only hold proxies).
// With a wrappers dir, only the wrappers and the fans' extras are built, into it, overwriting, and no map: for trying
// them in the running game with an FXR reloader.
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
  ProxyNode,
  State,
  StaticNodeTransform,
} from '@cccode/fxr'
import fs from 'node:fs'
import path from 'node:path'

// External values the DLL serves, red, green, blue each, as 1 + a 0..1 amount; 0 means no DLL.
const EXT_ADD = 900001 // what the add copy adds
const EXT_REMOVE = 900004 // what the remove copy subtracts
const EXT_PLAIN = 900007 // the plain color, for alpha-blended particles
// Whether the flame is on the player's AC (dll/src/player_flames.rs): 0 not known yet (or no DLL), 1 someone else's,
// 2 the player's.
const EXT_PLAYERS_FLAME = 900010
// The flame type the player chose (dll/src/thrusters.rs): 0 the generator's own, 1 + a type (0 combustion, 1 current,
// 2 Coral, then the same three as fans) otherwise.
const EXT_FLAME_TYPE = 900011
// Each of a row's flames, plain and as a fan.
const FLAME_TYPES_PER_ROW = 2
// The fan types: how the boss ALLMIND's thruster effects are built (f000642001: copies of one current-type nozzle
// flame, each on a node with an offset and a rotation, two rows fanning outward). The boss only has it with current
// flames; here every type gets one. The boss's five-flame layout: offsets in meters (x right, y up), rotations in
// degrees.
const FAN = [
  { offset: [0.65, 0.35, 0], rotation: [-10, 10, 0] },
  { offset: [0, 0.35, 0], rotation: [-10, 0, 0] },
  { offset: [-0.65, 0.35, 0], rotation: [-10, -10, 0] },
  { offset: [0.95, -0.2, 0], rotation: [0, 15, 0] },
  { offset: [-0.95, -0.2, 0], rotation: [0, -15, 0] },
]
// Its spacing and angles, scaled to an AC's nozzle. The longer a flame, the further apart a given angle puts the
// flames' ends, so the long ones get less. Rows are told apart by the family of their flames (their ID / 10000): 41
// regular, 43 quick boost, 45 assault boost.
const FAN_SCALE = 0.15
const FAN_ANGLE_SCALE = 0.75
const FAN_ANGLE_SCALE_LONG = 0.3
const LONG_FLAME_FAMILIES = new Set([43, 45])
// What the boss's two cluster effects have besides their flames (a wide glow and lights; the heavy one: heat
// distortion, a lens flare, more glow), by family, and their size next to the boss's. Other families get the fan
// alone: whether the extras would end with a quick boost's flame is untested.
const FAN_EXTRAS = { 41: 642001, 45: 642002 }
const FAN_EXTRAS_SCALE = 0.1
// Game value: the spawner's termination signal.
const EXT_TERMINATE = 0

const WRAPPER_OFFSET = 9000000
const ADD_OFFSET = 9100000
const REMOVE_OFFSET = 9200000
// The two copies of an effect: [ID offset, whether it's the remove copy].
const COPIES = [[ADD_OFFSET, false], [REMOVE_OFFSET, true]]

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
// the ParticleModifier's color (found with fxr-hunter). Everything else is only whitened. Lights and lens flares get
// the tint on their own colors. Distortion stays as it is: tinting it darkens the background into dark shapes.
// `remove`: additive particles subtract from the background instead, without bloom. Everything the add copy already
// draws is hidden: alpha-blended particles (subtracting them would darken their whole quad), lights, lens flares and
// distortion.
function recolor(fxr, remove) {
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
        addTint(action, key, EXT_ADD)
      }
    }
  }
  // Every particle: the tint on one factor (see above).
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
    if (!config.particleModifier) {
      config.particleModifier = new ParticleModifier()
    }
    let target = config.particleModifier
    let key = 'color'
    if (appearance.type === ActionType.Model) {
      target = appearance
      key = 'color2'
    }
    const additive = ADDITIVE.has(blendModeOf(appearance))
    if (!remove) {
      addTint(target, key, additive ? EXT_ADD : EXT_PLAIN)
    } else if (additive) {
      appearance.blendMode = BlendMode.Subtract
      if ('bloom' in appearance.$data.props) appearance.bloom = false
      addTint(target, key, EXT_REMOVE, 0)
    } else {
      target[key] = [1, 1, 1, 0]
    }
  }
}

// The wrapper for stock flame `stockId`, which is its row's flame for generator type `ownType`: a child node per effect
// it can play (a proxy to it). STOCK enables the stock flame; CUSTOM + a type enables both copies of the row's flame
// for that type; CUSTOM + the type count + a type enables that type's fan. The choice is made once, at spawn, and
// kept: a proxy's effect keeps playing after its node is disabled (switching from stock to custom left the stock
// flame running under the custom one), so a wrapper may only ever enable one of them. Conditions are checked in
// order, first match wins:
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
  const fanState = type => CUSTOM + rowFlames.length + type
  const customStates = FLAME_TYPES_PER_ROW * rowFlames.length
  // The served value is 1 + the state's place among the CUSTOM ones: highest first, since the first match wins.
  const pick = Array.from({ length: customStates }, (_, i) => `${flameType} >= ${i + 1} then ${CUSTOM + i}`).reverse()
  const states = [
    // DECIDING
    State.from([
      terminate,
      `External(${EXT_ADD}) < 1 then ${STOCK}`,
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
  const copies = (sfx, state) => COPIES.map(([offset]) => child(sfx + offset, state))
  const family = Math.floor(rowFlames[0] / 10000)
  const angleScale = LONG_FLAME_FAMILIES.has(family) ? FAN_ANGLE_SCALE_LONG : FAN_ANGLE_SCALE
  const extras = FAN_EXTRAS[family]
  // A type's fan: its flame at each of the layout's places, and the extras.
  const fan = (flame, type) => [
    ...FAN.map(({ offset, rotation }) =>
      inState(
        fanState(type),
        [
          new StaticNodeTransform({
            offset: offset.map(m => m * FAN_SCALE),
            rotation: rotation.map(degrees => degrees * angleScale),
          }),
        ],
        COPIES.map(([copy]) => new ProxyNode(flame + copy)),
      ),
    ),
    ...(extras ? copies(extras, fanState(type)) : []),
  ]
  return new FXR(stockId + WRAPPER_OFFSET, states, [
    child(stockId, STOCK),
    ...rowFlames.flatMap((flame, type) => copies(flame, CUSTOM + type)),
    ...rowFlames.flatMap(fan),
  ])
}

// The fan's extras: the boss's cluster effects without their flames (the nodes holding proxies), scaled down, as an
// add and a remove copy each.
const holdsProxy = node => node instanceof ProxyNode || (node.nodes ?? []).some(holdsProxy)
function withoutProxies(node) {
  if (!node.nodes) return
  node.nodes = node.nodes.filter(child => !holdsProxy(child))
  node.nodes.forEach(withoutProxies)
}
async function fanExtras() {
  const extras = []
  for (const extrasId of new Set(Object.values(FAN_EXTRAS))) {
    const source = path.join(allmindDir, fileName(extrasId))
    for (const [offset, remove] of COPIES) {
      const fxr = await FXR.read(source, Game.ArmoredCore6)
      fxr.id = extrasId + offset
      withoutProxies(fxr.root)
      fxr.root.scale(FAN_EXTRAS_SCALE)
      recolor(fxr, remove)
      extras.push({ fxr, source: extrasId })
    }
  }
  return extras
}

const [effectDir, idMapPath, rowsPath, allmindDir, wrappersDir] = process.argv.slice(2)
const rows = JSON.parse(fs.readFileSync(rowsPath, 'utf8'))
const stockIds = [...new Set(rows.flat())]
const fileName = id => `f${String(id).padStart(9, '0')}.fxr`
const file = id => path.join(effectDir, fileName(id))
// A path, not fs.readFileSync: small files come back as views into Node's shared buffer pool, which the library
// reads from the pool's start.
const read = id => FXR.read(file(id), Game.ArmoredCore6)
// The effect dir is a fresh unpack of the game's effects, so an existing file at a new ID is a stock effect. The
// wrappers dir is ours to overwrite.
async function save(fxr) {
  if (wrappersDir) return fxr.saveAs(path.join(wrappersDir, fileName(fxr.id)), Game.ArmoredCore6)
  if (fs.existsSync(file(fxr.id))) throw new Error(`${fxr.id} would overwrite an existing effect`)
  await fxr.saveAs(file(fxr.id), Game.ArmoredCore6)
}

// Every stock flame and everything it references through proxies gets both copies.
const resources = {}
const sources = new Set()
if (!wrappersDir) {
  const pending = [...stockIds]
  while (pending.length) {
    const id = pending.pop()
    if (sources.has(id)) continue
    sources.add(id)
    for (const node of (await read(id)).root.walk()) {
      if (node instanceof ProxyNode) pending.push(node.sfx)
    }
  }
  for (const sourceId of sources) {
    for (const [offset, remove] of COPIES) {
      const fxr = await read(sourceId)
      fxr.id = sourceId + offset
      for (const node of fxr.root.walk()) {
        if (node instanceof ProxyNode) node.sfx += offset
      }
      recolor(fxr, remove)
      await save(fxr)
      resources[fxr.id] = sourceId
    }
  }
}

const allmindResources = {}
for (const { fxr, source } of await fanExtras()) {
  await save(fxr)
  allmindResources[fxr.id] = source
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
  console.log(`${Object.keys(wrappers).length} wrappers in ${wrappersDir}`)
} else {
  const flameTypes = FLAME_TYPES_PER_ROW * rows[0].length
  fs.writeFileSync(idMapPath, JSON.stringify({ wrappers, resources, allmindResources, flameTypes }, null, 1))
  console.log(`${stockIds.length} wrappers, ${sources.size * 2} copies (${sources.size} sources)`)
}
