"""Generate deterministic pure-parser fixtures and an honest upstream test inventory.

Run in the pinned Python reference environment. Device I/O is simulated by the
upstream test suite; pure parsers are captured for exact cross-language replay.
Fixtures exercise the Python functions themselves, including exhaustive controller
bytes and voltage interpolation, valid seeds, truncation and seeded mutations.
The inventory preserves mappings; parser-call evidence alone is not full poll parity.
"""
import importlib
import json
import random
import sys
from pathlib import Path
import io
import contextlib
import unittest

ROOT = Path(__file__).resolve().parents[1]
REFERENCE_ROOT = ROOT.parent
sys.path.insert(0, str(REFERENCE_ROOT))
cases = []
rng = random.Random(1130)

def add(name, data, expected, charging=None):
    label=expected[2] if isinstance(expected,tuple) and len(expected)>2 and isinstance(expected[2],str) else None
    if isinstance(expected, tuple):
        expected, charging = expected[:2]
    cases.append(dict(parser=name, data=data, level=expected, charging=charging,label=label))

for module, function, name in [
    ('playstation', 'parse_ds4', 'ds4'),
    ('playstation', 'parse_dualsense', 'dualsense'),
    ('eightbitdo', 'parse_battery', 'eightbitdo'),
    ('nintendo', 'parse_battery', 'nintendo'),
]:
    f = getattr(importlib.import_module('providers.' + module), function)
    for value in range(256):
        add(name, [value], f(value))

for mv in range(2400, 4501):
    f = importlib.import_module('providers.logitech').voltage_to_percent
    add('voltage', [mv >> 8, mv & 255], f(mv))

specs = [
    ('audeze','find_level','audeze',[0xd6,0x0c,0,0,67]),
    ('wlmouse','parse_feature','wl_feature',[0xa1,0,0,2,0,0x83,1,67]),
    ('wlmouse','parse_heartbeat','wl_heartbeat',[3,0,67,1]),
    ('pulsar','parse_power','pulsar',[8,4,0,0,0,0,67,1,0,0,0,0,0,0,0,0,5]),
    ('mchose','parse_g7','mchose_g7',[0xaa,0x30,0xa5,0x0b,10,1,1,1,46,0]),
    ('mchose','parse_status','mchose',[0x11,0xf9,0xac,0xad,0xce,0xff,0,0,0,0,0xf6,188,254]),
    ('astro','parse_battery','astro',[2,12,0,0,6,0,67,0,1]),
    ('hyperx','parse_level','hyperx',[6,255,187,2,0,0,0,67]),
    ('hyperx_cloud3','parse_battery','hyperx3',[0x66,0x89,1,0,67]),
    ('hyperx_alpha2','parse_reply','hyperx_alpha',[0x51,2,67,0,0,0,0x80]),
    ('keychron','parse_level','keychron',[0xb4,6]+[0]*18+[67]),
    ('jbl','parse_level','jbl',[8,67]),
    ('am_infinity','parse_charge','am_infinity',[5,0,0,67]),
    ('corsair','parse_level','corsair',[0,0,0,0,0x9e,2]),
    ('corsair','parse_nxp','corsair_nxp',[0,0,0,0,3,0]),
] + [('steelseries',name,name,seed) for name,seed in [
    ('parse_nova7',[0xb0,3,67,1]),
    ('parse_nova7_discrete',[0xb0,3,3,1]),
    ('parse_nova5',[0xb0,3,0,67,1]),
    ('parse_arctis7_plus',[0xb0,3,3,1]),
    ('parse_gamebuds',[0xb0,0,0,3,3,67,50]),
    ('parse_rival3',[0xaa,67,0,1]),
    ('parse_aerox3',[0xd2,0x8c]),
]]
for scale in [1,25]:
    f = importlib.import_module('providers.asus').parse_reply
    seed=[0x12,7,0,0,3,0,0,0,0,1]
    for _ in range(200):
        data=seed.copy()
        data[rng.randrange(len(data))]=rng.randrange(256)
        add('asus'+str(scale),data,f(data,scale))
    for n in range(len(seed)+1):
        add('asus'+str(scale),seed[:n],f(seed[:n],scale))
for module, function, name, seed in specs:
    f = getattr(importlib.import_module('providers.'+module), function)
    buffers = [seed[:n] for n in range(len(seed)+1)] + [seed, [0]+seed]
    for _ in range(180):
        data = seed.copy()
        for _ in range(rng.randrange(1,4)):
            data[rng.randrange(len(data))] = rng.randrange(256)
        buffers.append(data)
    for data in buffers:
        expected = f(data)
        if module == 'corsair' and function == 'parse_nxp':
            label=None if expected is None else expected[1]
            expected = None if expected is None else expected[0]
            add(name,data,expected)
            cases[-1]['label']=label
            continue
        if module == 'astro':
            add(name,data,expected,importlib.import_module('providers.astro').parse_dock(data))
        else:
            add(name,data,expected)

for feature in [0x1000,0x1004,0x1001,0x1f20]:
    f = importlib.import_module('providers.logitech').parse_battery
    for _ in range(300):
        data = [rng.randrange(256) for _ in range(3)]
        result = f(feature,data)
        add('logitech',list(feature.to_bytes(2,'big'))+data,result)

wire_cases=[]
for module,barracuda in [('blackshark',False),('barracuda',True)]:
    m=importlib.import_module('providers.'+module)
    for command in [0x21,0x2a]:
        seed=[0]*64;seed[0]=m.REPORT_ID;index=m.REPLY_CMD_OFF;seed[index]=command;seed[index+1]=1;seed[index+2]=1;seed[index+3]=46
        buffers=[seed[:n] for n in range(65)]+[seed[1:]]
        for _ in range(500):
            data=seed.copy()
            for _ in range(rng.randrange(1,4)):
                data[rng.randrange(len(data))]=rng.randrange(256)
            buffers.append(data)
        for data in buffers:
            wire_cases.append(dict(barracuda=barracuda,command=command,data=data,payload=m.parse_reply(data,command)))
# Run the original fake-device suite and record its exact pure-parser calls as
# additional fixtures. Associated IDs are evidence links, not full poll parity.
current_test=None
recorded_tests={}
original_run=unittest.TestCase.run
def traced_run(self,result=None):
    global current_test
    previous=current_test;current_test=self.id()
    try:
        if current_test=='test_notifications.TextTests.test_real_low_battery_alert_uses_the_shared_text':
            # This upstream test lacks its usual fullscreen fake. Isolate the
            # current desktop just as HideRenameTestCase does for related tests.
            from unittest.mock import patch
            import test_hide_rename
            with patch.object(test_hide_rename.hb,'fullscreen_app_running',lambda:False):
                return original_run(self,result)
        return original_run(self,result)
    finally:
        current_test=previous
unittest.TestCase.run=traced_run
functions={(module,function):name for module,function,name,_ in specs}
functions.update({('asus','parse_reply'):'asus',('logitech','parse_battery'):'logitech',('logitech','voltage_to_percent'):'voltage',
                  ('playstation','parse_ds4'):'ds4',('playstation','parse_dualsense'):'dualsense',('eightbitdo','parse_battery'):'eightbitdo',('nintendo','parse_battery'):'nintendo'})
def tracer(original,name):
    def wrapped(*args,**kwargs):
        result=original(*args,**kwargs)
        if current_test:
            parser=name
            if name=='voltage':
                data=[args[0]>>8,args[0]&255]
            elif name=='logitech':
                data=list(args[0].to_bytes(2,'big'))+list(args[1])
            elif name in ('ds4','dualsense','eightbitdo','nintendo'):
                data=[args[0]]
            else:
                data=list(args[0] or [])
            if name=='asus':parser='asus'+str(args[1])
            expected=result
            if name=='corsair_nxp':expected=None if result is None else result[0]
            charging=None
            if name=='astro':charging=importlib.import_module('providers.astro').parse_dock(data)
            add(parser,data,expected,charging)
            if name=='corsair_nxp' and result is not None:cases[-1]['label']=result[1]
            cases[-1]['upstream_test']=current_test
            recorded_tests.setdefault(current_test,set()).add(parser)
        return result
    return wrapped
for (module,function),name in functions.items():
    m=importlib.import_module('providers.'+module)
    original=getattr(m,function)
    wrapped=tracer(original,name)
    setattr(m,function,wrapped)
    # Tables retain references to their parser; replace them together so their
    # identity assertions still check the same relationship.
    for attr in ['MODELS','MOUSE_MODELS','CLASSIC_MODELS']:
        table=getattr(m,attr,{})
        for key,value in table.items():
            if isinstance(value,tuple):
                table[key]=tuple(wrapped if item is original else item for item in value)
# Each upstream test replaces device I/O with fakes. The suite is preserved and
# all assertions must pass before captured calls become reusable Rust evidence.
reference_suite=unittest.defaultTestLoader.discover(str(REFERENCE_ROOT/'tests'))
with contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
    result=unittest.TextTestRunner(stream=io.StringIO()).run(reference_suite)
if not result.wasSuccessful():
    raise RuntimeError(f'reference fixture capture failed: {result.errors!r} {result.failures!r}')
dest = ROOT/'crates/providers/tests'
dest.mkdir(exist_ok=True)
(dest/'parser-fixtures.json').write_text(json.dumps(cases,indent=2)+'\n')
(dest/'pa-fixtures.json').write_text(json.dumps(wire_cases,indent=2)+'\n')
# unittest discovery is the upstream runner; flatten the collected suite without running it.
suite = unittest.defaultTestLoader.discover(str(REFERENCE_ROOT/'tests'))
inventory = []
def visit(suite):
    for test in suite:
        if isinstance(test, unittest.TestSuite):
            visit(test)
        else:
            inventory.append(dict(upstream_test=test.id(),status='not_mapped',rust_test=None))
visit(suite)
inventory_path=ROOT/'docs/provider-test-inventory.json'
previous={row['upstream_test']:row for row in json.loads(inventory_path.read_text())} if inventory_path.exists() else {}
for row in inventory:
    if row['upstream_test'] in previous:
        row.update(previous[row['upstream_test']])
for row in inventory:
    if row['upstream_test'] in recorded_tests:
        row['parser_evidence']={'rust_test':'crates/providers/tests/parser_parity.rs::python_reference_parser_fixtures','parsers':sorted(recorded_tests[row['upstream_test']])}
inventory_path.write_text(json.dumps(inventory,indent=2)+'\n')
print(f'{len(cases)} parser cases; {len(wire_cases)} PA cases; {len(inventory)} upstream tests inventoried; {len(recorded_tests)} IDs have exact parser-call evidence')
