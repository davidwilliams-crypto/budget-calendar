"""Stack-simulating pseudo-decompiler for Java class files. Output is Java-like
pseudo code with explicit labels/gotos. Written for reading, not compiling."""
import struct, sys, os, re, json, glob
sys.path.insert(0, os.path.dirname(__file__))
import jclass
from jclass import CF, OPS, parse_code

PKG_STRIP = ['ca/mishell/app/budget/', 'ca/mishell/misc/', 'ca/mishell/ui/config/', 'ca/mishell/ui/',
             'java/lang/', 'java/util/', 'java/io/', 'org/eclipse/swt/widgets/', 'org/eclipse/swt/graphics/',
             'org/eclipse/swt/events/', 'org/eclipse/swt/custom/', 'org/eclipse/swt/', 'java/util/function/', 'java/text/']

def short(cls):
    for p in PKG_STRIP:
        if cls.startswith(p): return cls[len(p):].replace('/', '.')
    return cls.replace('/', '.')

def desc2type(d, i=0):
    """returns (typestring, next_index)"""
    c = d[i]
    if c == 'L':
        j = d.index(';', i); return short(d[i+1:j]), j+1
    if c == '[':
        t, j = desc2type(d, i+1); return t+'[]', j
    return {'I':'int','J':'long','D':'double','F':'float','Z':'boolean','B':'byte','C':'char','S':'short','V':'void'}[c], i+1

def parse_method_desc(d):
    assert d[0] == '('
    i = 1; params = []
    while d[i] != ')':
        t, i = desc2type(d, i); params.append(t)
    ret, _ = desc2type(d, i+1)
    return params, ret

def wide_type(t): return t in ('long', 'double')

class Decomp:
    def __init__(self, cf):
        self.cf = cf
        self.bsm = self.parse_bootstrap()
        self.counter = 0

    def parse_bootstrap(self):
        out = []
        for b in self.cf.attrs.get('BootstrapMethods', []):
            n = struct.unpack('>H', b[:2])[0]; p = 2
            for _ in range(n):
                ref, na = struct.unpack('>HH', b[p:p+4]); p += 4
                args = [struct.unpack('>H', b[p+2*i:p+2*i+2])[0] for i in range(na)]; p += 2*na
                out.append((ref, args))
        return out

    def cst(self, i):
        e = self.cf.cp[i]; t = e[0]
        if t == 'String': return json.dumps(self.cf.utf(e[1]))
        if t == 'Class': return short(self.cf.cls(i))
        if t == 'Int': return str(e[1])
        if t == 'Long': return str(e[1]) + 'L'
        if t == 'Float': return repr(e[1]) + 'f'
        if t == 'Double': return repr(e[1])
        return self.cf.const(i)

    def fieldref(self, i):
        e = self.cf.cp[i]; c = self.cf.cls(e[1]); n, t = self.cf.nt(e[2]); return short(c), n, t

    def methodref(self, i):
        e = self.cf.cp[i]; c = self.cf.cls(e[1]); n, t = self.cf.nt(e[2]); return short(c), n, t

    def indy(self, i):
        e = self.cf.cp[i]; bsm_idx = e[1]; n, t = self.cf.nt(e[2])
        ref, args = self.bsm[bsm_idx]
        mh = self.cf.cp[ref]; target = self.cf.const(mh[2])
        return n, t, target, args

    def method(self, m):
        cf = self.cf
        params, ret = parse_method_desc(m['desc'])
        is_static = bool(m['acc'] & 8)
        names = {}
        slot = 0
        if not is_static: names[0] = 'this'; slot = 1
        plist = []
        for k, t in enumerate(params):
            names[slot] = 'p%d' % k; plist.append('%s p%d' % (t, k)); slot += 2 if wide_type(t) else 1
        head = '%s %s %s(%s)' % (jclass.flags(m['acc']), ret, m['name'], ', '.join(plist))
        c = parse_code(cf, m)
        if not c: return head + ';'
        for idx, n, d, _, _ in c['lvt']:
            if idx not in names: names[idx] = n
        self.names = names
        lines = self.body(c)
        return head + ' {\n' + '\n'.join('    ' + l for l in lines) + '\n}'

    def local(self, i):
        return self.names.get(i, 'v%d' % i)

    def body(self, c):
        code = bytes(c['code'])
        # first pass: decode instructions
        insns = []  # (addr, name, kind, arg(s))
        p = 0
        while p < len(code):
            op = code[p]; start = p; p += 1
            name, n = OPS[op]
            arg = None
            if n == 0: pass
            elif n == 1:
                v = code[p]; p += 1
                if name == 'bipush': arg = struct.unpack('b', bytes([v]))[0]
                else: arg = v
            elif n == 2:
                if name == 'sipush': arg = struct.unpack('>h', code[p:p+2])[0]
                elif name == 'iinc': arg = (code[p], struct.unpack('b', code[p+1:p+2])[0])
                else: arg = struct.unpack('>H', code[p:p+2])[0]
                p += 2
            elif n == 'br2': arg = start + struct.unpack('>h', code[p:p+2])[0]; p += 2
            elif n == 'br4': arg = start + struct.unpack('>i', code[p:p+4])[0]; p += 4
            elif n == 4: arg = struct.unpack('>H', code[p:p+2])[0]; p += 4
            elif n == 3: arg = (struct.unpack('>H', code[p:p+2])[0], code[p+2]); p += 3
            elif n == 'wide':
                op2 = code[p]; p += 1; nm = OPS[op2][0]; idx = struct.unpack('>H', code[p:p+2])[0]; p += 2
                if op2 == 132: cst = struct.unpack('>h', code[p:p+2])[0]; p += 2; name = 'iinc'; arg = (idx, cst)
                else: name = nm; arg = idx
            elif n == 'ts':
                while p % 4: p += 1
                dflt, lo, hi = struct.unpack('>iii', code[p:p+12]); p += 12
                offs = [struct.unpack('>i', code[p+4*i:p+4*i+4])[0] for i in range(hi-lo+1)]; p += 4*(hi-lo+1)
                arg = ([(lo+i, start+o) for i, o in enumerate(offs)], start+dflt)
            elif n == 'ls':
                while p % 4: p += 1
                dflt, npairs = struct.unpack('>ii', code[p:p+8]); p += 8
                pairs = [struct.unpack('>ii', code[p+8*i:p+8*i+8]) for i in range(npairs)]; p += 8*npairs
                arg = ([(k, start+o) for k, o in pairs], start+dflt)
            insns.append((start, name, arg, p))
        # leaders
        targets = set()
        for a, name, arg, nxt in insns:
            if name in ('goto', 'goto_w') or name.startswith('if'):
                targets.add(arg)
            if name in ('tableswitch', 'lookupswitch'):
                for _, t in arg[0]: targets.add(t)
                targets.add(arg[1])
        handlers = {}
        for s, e, h, t in c['exc']:
            targets.add(h); handlers.setdefault(h, []).append((s, e, t))
        out = []
        stack = []
        incoming = {}  # addr -> depth
        self.counter = 0
        new_placeholders = {}

        def spill(depth_target):
            # assign stack to $sN and return
            nonlocal stack
            if not stack: return
            for i, v in enumerate(stack):
                if v != '$s%d' % i: out.append('$s%d = %s;' % (i, v))
            incoming[depth_target] = len(stack)

        wide = []  # parallel to stack: True for long/double
        def push(v, w=False):
            stack.append(v); wide.append(w)
        def pop():
            if not stack:
                return '<?stack-underflow?>'
            wide.pop(); return stack.pop()
        def top_wide():
            return wide[-1] if wide else False
        def materialize(v):
            # side-effecting expression duplicated on the stack -> name it once
            if '(' in v and not v.startswith('#new') and not v.startswith('$'):
                self.counter += 1; t = '$t%d' % self.counter
                out.append('%s = %s;' % (t, v)); return t
            return v

        def cond(name, a, b=None):
            ops = {'eq': '==', 'ne': '!=', 'lt': '<', 'ge': '>=', 'gt': '>', 'le': '<='}
            if name.startswith('if_icmp') or name.startswith('if_acmp'):
                return '%s %s %s' % (a, ops[name[-2:]], b)
            if name == 'ifnull': return '%s == null' % a
            if name == 'ifnonnull': return '%s != null' % a
            o = ops[name[2:]]
            m = re.match(r'^cmp\((.*)\)$', a)
            if m and a.startswith('cmp('):
                # fused compare: find top-level comma
                inner = a[4:-1]; depth = 0
                for i, ch in enumerate(inner):
                    if ch in '([': depth += 1
                    elif ch in ')]': depth -= 1
                    elif ch == ',' and depth == 0:
                        return '%s %s %s' % (inner[:i].strip(), o, inner[i+1:].strip())
            if o == '!=' : return '%s' % a if a.endswith(')') and False else '%s != 0' % a
            if o == '==': return '%s == 0' % a
            return '%s %s 0' % (a, o)

        def invoke(name, idx, is_iface=False):
            cls, mn, desc = self.methodref(idx)
            params, ret = parse_method_desc(desc)
            args = [pop() for _ in params][::-1]
            if name == 'invokestatic':
                expr = '%s.%s(%s)' % (cls, mn, ', '.join(args))
            else:
                recv = pop()
                if mn == '<init>':
                    if recv.startswith('#new'):
                        full = 'new %s(%s)' % (new_placeholders[recv], ', '.join(args))
                        # replace placeholder in stack
                        for i, v in enumerate(stack):
                            if v == recv: stack[i] = full
                        # if no reference remained on stack, the object was discarded: emit stmt
                        if recv not in stack and full not in stack:
                            out.append(full + ';')
                        return
                    out.append('%s.<init>(%s);' % ('super' if recv == 'this' else recv, ', '.join(args)))
                    return
                if name == 'invokespecial' and recv == 'this' and cls != short(self.cf.this):
                    expr = 'super.%s(%s)' % (mn, ', '.join(args))
                else:
                    expr = '%s.%s(%s)' % (recv, mn, ', '.join(args))
            if ret == 'void':
                out.append(expr + ';')
            else:
                push(expr, wide_type(ret))

        for a, name, arg, nxt in insns:
            if a in targets:
                if stack:
                    spill(a)
                if a in handlers:
                    stack = ['$ex']; wide[:] = [False]
                    for s, e, t in handlers[a]:
                        out.append('// handler for [%d,%d) catch %s' % (s, e, short(t) if t != 'any' else 'any'))
                else:
                    d = incoming.get(a, 0)
                    stack = ['$s%d' % i for i in range(d)]; wide[:] = [False]*d
                out.append('L%d:' % a)
            # ---- instruction semantics
            if name == 'nop': pass
            elif name == 'aconst_null': push('null')
            elif name.startswith('iconst_'): push(name[7:].replace('m1', '-1'))
            elif name.startswith('lconst_'): push(name[7:] + 'L', True)
            elif name.startswith('fconst_'): push(name[7:] + '.0f')
            elif name.startswith('dconst_'): push(name[7:] + '.0', True)
            elif name in ('bipush', 'sipush'): push(str(arg))
            elif name in ('ldc', 'ldc_w', 'ldc2_w'): push(self.cst(arg), name == 'ldc2_w')
            elif re.match(r'^[ilfda]load(_\d)?$', name):
                i = arg if arg is not None else int(name[-1]); push(self.local(i), name[0] in 'ld')
            elif re.match(r'^[ilfda]store(_\d)?$', name):
                i = arg if arg is not None else int(name[-1]); v = pop()
                out.append('%s = %s;' % (self.local(i), v))
            elif name in ('iaload', 'laload', 'faload', 'daload', 'aaload', 'baload', 'caload', 'saload'):
                i = pop(); arr = pop(); push('%s[%s]' % (arr, i), name[0] in 'ld')
            elif name in ('iastore', 'lastore', 'fastore', 'dastore', 'aastore', 'bastore', 'castore', 'sastore'):
                v = pop(); i = pop(); arr = pop(); out.append('%s[%s] = %s;' % (arr, i, v))
            elif name == 'pop':
                v = pop()
                if '(' in v: out.append(v + ';')
            elif name == 'pop2':
                v = pop()
                if '(' in v: out.append(v + ';')
            elif name == 'dup':
                w = top_wide(); v = materialize(pop()); push(v, w); push(v, w)
            elif name == 'dup_x1':
                w1 = top_wide(); v1 = materialize(pop()); w2 = top_wide(); v2 = pop(); push(v1, w1); push(v2, w2); push(v1, w1)
            elif name == 'dup_x2':
                w1 = top_wide(); v1 = materialize(pop()); w2 = top_wide(); v2 = pop()
                if w2:
                    push(v1, w1); push(v2, w2); push(v1, w1)
                else:
                    w3 = top_wide(); v3 = pop(); push(v1, w1); push(v3, w3); push(v2, w2); push(v1, w1)
            elif name == 'dup2':
                if top_wide():
                    v1 = materialize(pop()); push(v1, True); push(v1, True)
                else:
                    v1 = materialize(pop()); v2 = materialize(pop()); push(v2); push(v1); push(v2); push(v1)
            elif name == 'dup2_x1':
                if top_wide():
                    v1 = materialize(pop()); w2 = top_wide(); v2 = pop(); push(v1, True); push(v2, w2); push(v1, True)
                else:
                    v1 = materialize(pop()); v2 = materialize(pop()); w3 = top_wide(); v3 = pop(); push(v2); push(v1); push(v3, w3); push(v2); push(v1)
            elif name == 'dup2_x2':
                if top_wide():
                    v1 = materialize(pop()); w2 = top_wide(); v2 = pop()
                    if w2:
                        push(v1, True); push(v2, True); push(v1, True)
                    else:
                        w3 = top_wide(); v3 = pop(); push(v1, True); push(v3, w3); push(v2, w2); push(v1, True)
                else:
                    v1 = materialize(pop()); v2 = materialize(pop()); w3 = top_wide(); v3 = pop()
                    if w3:
                        push(v2); push(v1); push(v3, w3); push(v2); push(v1)
                    else:
                        w4 = top_wide(); v4 = pop(); push(v2); push(v1); push(v4, w4); push(v3, w3); push(v2); push(v1)
            elif name == 'swap':
                v1 = pop(); v2 = pop(); push(v1); push(v2)
            elif name[1:] in ('add', 'sub', 'mul', 'div', 'rem', 'shl', 'shr', 'ushr', 'and', 'or', 'xor') and name[0] in 'ilfd':
                o = {'add': '+', 'sub': '-', 'mul': '*', 'div': '/', 'rem': '%', 'shl': '<<', 'shr': '>>', 'ushr': '>>>', 'and': '&', 'or': '|', 'xor': '^'}[name[1:]]
                b = pop(); a2 = pop(); push('(%s %s %s)' % (a2, o, b), name[0] in 'ld')
            elif name[1:] == 'neg': push('(-%s)' % pop(), name[0] in 'ld')
            elif name in ('i2l', 'i2f', 'i2d', 'l2i', 'l2f', 'l2d', 'f2i', 'f2l', 'f2d', 'd2i', 'd2l', 'd2f', 'i2b', 'i2c', 'i2s'):
                t = {'l': 'long', 'f': 'float', 'd': 'double', 'i': 'int', 'b': 'byte', 'c': 'char', 's': 'short'}[name[2]]
                push('(%s)%s' % (t, pop()), name[2] in 'ld')
            elif name in ('lcmp', 'fcmpl', 'fcmpg', 'dcmpl', 'dcmpg'):
                b = pop(); a2 = pop(); push('cmp(%s, %s)' % (a2, b))
            elif name.startswith('if'):
                if name.startswith('if_'):
                    b = pop(); a2 = pop(); cnd = cond(name, a2, b)
                else:
                    a2 = pop(); cnd = cond(name, a2)
                spill(arg)
                out.append('if (%s) goto L%d;' % (cnd, arg))
            elif name in ('goto', 'goto_w'):
                spill(arg)
                out.append('goto L%d;' % arg)
                stack = []; wide[:] = []
            elif name in ('tableswitch', 'lookupswitch'):
                v = pop()
                cases = ', '.join('%d: L%d' % (k, t) for k, t in arg[0])
                out.append('switch (%s) { %s; default: L%d }' % (v, cases, arg[1]))
                stack = []; wide[:] = []
            elif name in ('ireturn', 'lreturn', 'freturn', 'dreturn', 'areturn'):
                out.append('return %s;' % pop()); stack = []; wide[:] = []
            elif name == 'return':
                out.append('return;'); stack = []; wide[:] = []
            elif name == 'getstatic':
                cls, f, t = self.fieldref(arg); push('%s.%s' % (cls, f), t in 'JD')
            elif name == 'putstatic':
                cls, f, t = self.fieldref(arg); out.append('%s.%s = %s;' % (cls, f, pop()))
            elif name == 'getfield':
                cls, f, t = self.fieldref(arg); o = pop(); push('%s.%s' % (o, f), t in 'JD')
            elif name == 'putfield':
                cls, f, t = self.fieldref(arg); v = pop(); o = pop(); out.append('%s.%s = %s;' % (o, f, v))
            elif name in ('invokevirtual', 'invokespecial', 'invokestatic', 'invokeinterface'):
                invoke(name, arg)
            elif name == 'invokedynamic':
                n, t, target, bargs = self.indy(arg)
                params, ret = parse_method_desc(t)
                args = [pop() for _ in params][::-1]
                if n == 'makeConcatWithConstants':
                    recipe = self.cf.utf(self.cf.cp[bargs[0]][1])
                    parts = []; ai = 0; buf = ''
                    for ch in recipe:
                        if ch == '\x01':
                            if buf: parts.append(json.dumps(buf)); buf = ''
                            parts.append(args[ai]); ai += 1
                        else: buf += ch
                    if buf: parts.append(json.dumps(buf))
                    push('(' + ' + '.join(parts) + ')')
                elif 'LambdaMetafactory' in target:
                    impl = self.cf.const(bargs[1]).replace('mh ', '')
                    m = re.match(r'^(.*)\.(.*):\(', impl)
                    impl_s = short(m.group(1)) + '::' + m.group(2) if m else impl
                    push('lambda[%s](%s)' % (impl_s, ', '.join(args)))
                else:
                    push('indy %s(%s)' % (n, ', '.join(args)))
            elif name == 'new':
                self.counter += 1; ph = '#new%d' % self.counter
                new_placeholders[ph] = short(self.cf.cls(arg)); push(ph)
            elif name == 'newarray':
                t = {4: 'boolean', 5: 'char', 6: 'float', 7: 'double', 8: 'byte', 9: 'short', 10: 'int', 11: 'long'}[arg]
                self.counter += 1; nm = '$a%d' % self.counter
                out.append('%s = new %s[%s];' % (nm, t, pop())); push(nm)
            elif name == 'anewarray':
                self.counter += 1; nm = '$a%d' % self.counter
                out.append('%s = new %s[%s];' % (nm, short(self.cf.cls(arg)), pop())); push(nm)
            elif name == 'multianewarray':
                dims = [pop() for _ in range(arg[1])][::-1]
                push('new %s%s' % (short(self.cf.cls(arg[0])).replace('[]', ''), ''.join('[%s]' % d for d in dims)))
            elif name == 'arraylength': push('%s.length' % pop())
            elif name == 'athrow': out.append('throw %s;' % pop()); stack = []; wide[:] = []
            elif name == 'checkcast': push('(%s)%s' % (short(self.cf.cls(arg)), pop()))
            elif name == 'instanceof': push('(%s instanceof %s)' % (pop(), short(self.cf.cls(arg))))
            elif name in ('monitorenter', 'monitorexit'): out.append('%s(%s);' % (name, pop()))
            elif name == 'iinc':
                ln = self.local(arg[0])
                pat = r'\b%s\b' % re.escape(ln)
                if any(re.search(pat, sv) for sv in stack):
                    self.counter += 1; t = '$t%d' % self.counter
                    out.append('%s = %s;  // value before increment' % (t, ln))
                    for si, sv in enumerate(stack):
                        stack[si] = re.sub(pat, t, sv)
                out.append('%s += %d;' % (ln, arg[1]))
            elif name in ('jsr', 'ret', 'jsr_w'): out.append('// %s' % name)
            else:
                out.append('// ?? %s' % name)
        # post: resolve any leftover placeholders
        res = []
        for l in out:
            for ph, t in new_placeholders.items():
                l = l.replace(ph, 'new %s(?)' % t)
            res.append(l)
        return res

    def dump_class(self):
        cf = self.cf
        out = ['// ===== %s extends %s %s' % (short(cf.this), short(cf.super) if cf.super else '', ('implements ' + ', '.join(short(i) for i in cf.ifaces)) if cf.ifaces else '')]
        # constant static finals
        for f in cf.fields:
            cv = ''
            if 'ConstantValue' in f['attrs']:
                idx = struct.unpack('>H', f['attrs']['ConstantValue'][0])[0]; cv = ' = ' + self.cst(idx)
            t, _ = desc2type(f['desc'])
            out.append('%s %s %s%s;' % (jclass.flags(f['acc']), t, f['name'], cv))
        for m in cf.methods:
            out.append('')
            out.append(self.method(m))
        return '\n'.join(out)

if __name__ == '__main__':
    src, dst = sys.argv[1], sys.argv[2]
    os.makedirs(dst, exist_ok=True)
    files = sorted(glob.glob(os.path.join(src, '**', '*.class'), recursive=True))
    groups = {}
    for f in files:
        rel = os.path.relpath(f, src).replace('\\', '/')
        outer = re.sub(r'\$.*', '', rel[:-6])
        groups.setdefault(outer, []).append(f)
    for outer, fl in groups.items():
        chunks = []
        for f in sorted(fl):
            try:
                chunks.append(Decomp(CF(open(f, 'rb').read())).dump_class())
            except Exception as e:
                chunks.append('// FAILED %s: %r' % (f, e))
        op = os.path.join(dst, outer.replace('/', '.') + '.txt')
        open(op, 'w', encoding='utf-8').write('\n\n'.join(chunks))
        print(op, sum(len(c) for c in chunks))
