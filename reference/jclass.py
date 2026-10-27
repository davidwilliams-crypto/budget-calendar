import struct, sys, os, json

OPS = {}
def _op(code, name, n=0): OPS[code]=(name,n)
simple = """0 nop|1 aconst_null|2 iconst_m1|3 iconst_0|4 iconst_1|5 iconst_2|6 iconst_3|7 iconst_4|8 iconst_5|9 lconst_0|10 lconst_1|11 fconst_0|12 fconst_1|13 fconst_2|14 dconst_0|15 dconst_1|46 iaload|47 laload|48 faload|49 daload|50 aaload|51 baload|52 caload|53 saload|79 iastore|80 lastore|81 fastore|82 dastore|83 aastore|84 bastore|85 castore|86 sastore|87 pop|88 pop2|89 dup|90 dup_x1|91 dup_x2|92 dup2|93 dup2_x1|94 dup2_x2|95 swap|96 iadd|97 ladd|98 fadd|99 dadd|100 isub|101 lsub|102 fsub|103 dsub|104 imul|105 lmul|106 fmul|107 dmul|108 idiv|109 ldiv|110 fdiv|111 ddiv|112 irem|113 lrem|114 frem|115 drem|116 ineg|117 lneg|118 fneg|119 dneg|120 ishl|121 lshl|122 ishr|123 lshr|124 iushr|125 lushr|126 iand|127 land|128 ior|129 lor|130 ixor|131 lxor|133 i2l|134 i2f|135 i2d|136 l2i|137 l2f|138 l2d|139 f2i|140 f2l|141 f2d|142 d2i|143 d2l|144 d2f|145 i2b|146 i2c|147 i2s|148 lcmp|149 fcmpl|150 fcmpg|151 dcmpl|152 dcmpg|172 ireturn|173 lreturn|174 freturn|175 dreturn|176 areturn|177 return|190 arraylength|191 athrow|194 monitorenter|195 monitorexit"""
for e in simple.split('|'):
    c,n=e.split(); _op(int(c),n)
for base,name in [(26,'iload'),(30,'lload'),(34,'fload'),(38,'dload'),(42,'aload'),(59,'istore'),(63,'lstore'),(67,'fstore'),(71,'dstore'),(75,'astore')]:
    for i in range(4): _op(base+i, name+'_'+str(i))
for c,n in [(16,'bipush'),(21,'iload'),(22,'lload'),(23,'fload'),(24,'dload'),(25,'aload'),(54,'istore'),(55,'lstore'),(56,'fstore'),(57,'dstore'),(58,'astore'),(169,'ret'),(188,'newarray')]: _op(c,n,1)
_op(18,'ldc',1)
for c,n in [(17,'sipush'),(19,'ldc_w'),(20,'ldc2_w'),(132,'iinc'),(178,'getstatic'),(179,'putstatic'),(180,'getfield'),(181,'putfield'),(182,'invokevirtual'),(183,'invokespecial'),(184,'invokestatic'),(187,'new'),(189,'anewarray'),(192,'checkcast'),(193,'instanceof')]: _op(c,n,2)
for c,n in [(153,'ifeq'),(154,'ifne'),(155,'iflt'),(156,'ifge'),(157,'ifgt'),(158,'ifle'),(159,'if_icmpeq'),(160,'if_icmpne'),(161,'if_icmplt'),(162,'if_icmpge'),(163,'if_icmpgt'),(164,'if_icmple'),(165,'if_acmpeq'),(166,'if_acmpne'),(167,'goto'),(168,'jsr'),(198,'ifnull'),(199,'ifnonnull')]: _op(c,n,'br2')
_op(185,'invokeinterface',4); _op(186,'invokedynamic',4); _op(197,'multianewarray',3)
_op(200,'goto_w','br4'); _op(201,'jsr_w','br4'); _op(196,'wide','wide'); _op(170,'tableswitch','ts'); _op(171,'lookupswitch','ls')
SUFFIX={"Long":"L","Float":"f","Double":"d","Int":""}

class CF:
    def __init__(self, data):
        self.d=data; self.p=0
        assert self.u4()==0xCAFEBABE
        self.minor=self.u2(); self.major=self.u2()
        n=self.u2(); self.cp=[None]*n; i=1
        while i<n:
            t=self.u1()
            if t==1:
                l=self.u2(); self.cp[i]=('Utf8', self.d[self.p:self.p+l].decode('utf-8','replace')); self.p+=l
            elif t==3: self.cp[i]=('Int', struct.unpack('>i',self.rd(4))[0])
            elif t==4: self.cp[i]=('Float', struct.unpack('>f',self.rd(4))[0])
            elif t==5: self.cp[i]=('Long', struct.unpack('>q',self.rd(8))[0]); i+=1
            elif t==6: self.cp[i]=('Double', struct.unpack('>d',self.rd(8))[0]); i+=1
            elif t==7: self.cp[i]=('Class', self.u2())
            elif t==8: self.cp[i]=('String', self.u2())
            elif t in (9,10,11): self.cp[i]=({9:'Field',10:'Method',11:'IMethod'}[t], self.u2(), self.u2())
            elif t==12: self.cp[i]=('NameType', self.u2(), self.u2())
            elif t==15: self.cp[i]=('MHandle', self.u1(), self.u2())
            elif t==16: self.cp[i]=('MType', self.u2())
            elif t==17: self.cp[i]=('Dynamic', self.u2(), self.u2())
            elif t==18: self.cp[i]=('InvokeDynamic', self.u2(), self.u2())
            elif t==19: self.cp[i]=('Module', self.u2())
            elif t==20: self.cp[i]=('Package', self.u2())
            else: raise Exception('cp tag %d'%t)
            i+=1
        self.access=self.u2(); self.this=self.cls(self.u2()); s=self.u2(); self.super=self.cls(s) if s else None
        self.ifaces=[self.cls(self.u2()) for _ in range(self.u2())]
        self.fields=[self.member() for _ in range(self.u2())]
        self.methods=[self.member() for _ in range(self.u2())]
        self.attrs=self.attributes()
    def rd(self,n): r=self.d[self.p:self.p+n]; self.p+=n; return r
    def u1(self): return self.rd(1)[0]
    def u2(self): return struct.unpack('>H',self.rd(2))[0]
    def u4(self): return struct.unpack('>I',self.rd(4))[0]
    def utf(self,i): return self.cp[i][1]
    def cls(self,i): return self.utf(self.cp[i][1])
    def nt(self,i): e=self.cp[i]; return self.utf(e[1]), self.utf(e[2])
    def ref(self,i):
        e=self.cp[i]; c=self.cls(e[1]); n,t=self.nt(e[2]); return '%s.%s:%s'%(c,n,t)
    def const(self,i):
        e=self.cp[i]; t=e[0]
        if t=='String': return json.dumps(self.utf(e[1]))
        if t=='Class': return self.cls(i)
        if t in('Int','Float','Long','Double'): return str(e[1])+SUFFIX[t]
        if t in ('Field','Method','IMethod'): return self.ref(i)
        if t=='InvokeDynamic': n,ty=self.nt(e[2]); return 'indy#%d %s:%s'%(e[1],n,ty)
        if t=='NameType': n,ty=self.nt(i); return n+':'+ty
        if t=='MHandle': return 'mh '+self.const(e[2])
        return str(e)
    def attributes(self):
        out={}
        for _ in range(self.u2()):
            name=self.utf(self.u2()); l=self.u4(); out.setdefault(name,[]).append(self.rd(l))
        return out
    def member(self):
        acc=self.u2(); name=self.utf(self.u2()); desc=self.utf(self.u2()); attrs=self.attributes()
        return dict(acc=acc,name=name,desc=desc,attrs=attrs)

def flags(a):
    f=[]
    for bit,n in [(1,'public'),(2,'private'),(4,'protected'),(8,'static'),(16,'final'),(32,'synchronized'),(64,'volatile/bridge'),(128,'transient/varargs'),(256,'native'),(512,'interface'),(1024,'abstract'),(4096,'synthetic'),(16384,'enum')]:
        if a&bit: f.append(n)
    return ' '.join(f)

def disasm(cf, code):
    p=0; out=[]
    code=bytes(code)
    while p<len(code):
        op=code[p]; start=p; p+=1
        if op not in OPS: out.append('%d: ??%d'%(start,op)); continue
        name,n=OPS[op]; arg=''
        if n==0: pass
        elif n==1:
            v=code[p]; p+=1
            if name=='ldc': arg=cf.const(v)
            elif name=='bipush': arg=str(struct.unpack('b',bytes([v]))[0])
            elif name=='newarray': arg={4:'boolean',5:'char',6:'float',7:'double',8:'byte',9:'short',10:'int',11:'long'}.get(v,str(v))
            else: arg=str(v)
        elif n==2:
            v=struct.unpack('>H',code[p:p+2])[0]; p+=2
            if name=='sipush': arg=str(struct.unpack('>h',code[start+1:start+3])[0])
            elif name=='iinc': arg='%d %d'%(code[start+1],struct.unpack('b',code[start+2:start+3])[0])
            else: arg=cf.const(v)
        elif n=='br2':
            v=struct.unpack('>h',code[p:p+2])[0]; p+=2; arg='-> %d'%(start+v)
        elif n=='br4':
            v=struct.unpack('>i',code[p:p+4])[0]; p+=4; arg='-> %d'%(start+v)
        elif n==4:
            v=struct.unpack('>H',code[p:p+2])[0]; p+=4; arg=cf.const(v)
        elif n==3:
            v=struct.unpack('>H',code[p:p+2])[0]; p+=3; arg='%s dims=%d'%(cf.const(v),code[p-1])
        elif n=='wide':
            op2=code[p]; p+=1; nm=OPS[op2][0]; idx=struct.unpack('>H',code[p:p+2])[0]; p+=2
            if op2==132: c=struct.unpack('>h',code[p:p+2])[0]; p+=2; arg='%s %d %d'%(nm,idx,c)
            else: arg='%s %d'%(nm,idx)
        elif n=='ts':
            while p%4: p+=1
            dflt,lo,hi=struct.unpack('>iii',code[p:p+12]); p+=12
            offs=[struct.unpack('>i',code[p+4*i:p+4*i+4])[0] for i in range(hi-lo+1)]; p+=4*(hi-lo+1)
            arg='{'+', '.join('%d->%d'%(lo+i,start+o) for i,o in enumerate(offs))+', default->%d}'%(start+dflt)
        elif n=='ls':
            while p%4: p+=1
            dflt,npairs=struct.unpack('>ii',code[p:p+8]); p+=8
            pairs=[struct.unpack('>ii',code[p+8*i:p+8*i+8]) for i in range(npairs)]; p+=8*npairs
            arg='{'+', '.join('%d->%d'%(k,start+o) for k,o in pairs)+', default->%d}'%(start+dflt)
        out.append(('%d: %s %s'%(start,name,arg)).rstrip())
    return out

def parse_code(cf, m):
    if 'Code' not in m['attrs']: return None
    a=m['attrs']['Code'][0]
    ms,ml,cl=struct.unpack('>HHI',a[:8]); code=a[8:8+cl]; p=8+cl
    ne=struct.unpack('>H',a[p:p+2])[0]; p+=2
    exc=[]
    for _ in range(ne):
        s,e,h,t=struct.unpack('>HHHH',a[p:p+8]); p+=8; exc.append((s,e,h,cf.cls(t) if t else 'any'))
    sub=CF.__new__(CF); sub.d=a; sub.p=p; sub.cp=cf.cp
    attrs=CF.attributes(sub)
    lvt=[]
    for b in attrs.get('LocalVariableTable',[]):
        n=struct.unpack('>H',b[:2])[0]
        for i in range(n):
            st,ln,ni,di,idx=struct.unpack('>HHHHH',b[2+10*i:12+10*i]); lvt.append((idx,cf.utf(ni),cf.utf(di),st,ln))
    return dict(maxstack=ms,maxlocals=ml,code=code,exc=exc,lvt=lvt,attrs=list(attrs.keys()))

def dump(path, want_code=True, only=None):
    cf=CF(open(path,'rb').read())
    out=['// %s major=%d'%(path,cf.major)]
    out.append('class %s extends %s implements %s [%s]'%(cf.this,cf.super,cf.ifaces,flags(cf.access)))
    for f in cf.fields: out.append('  field %s %s : %s'%(flags(f["acc"]),f["name"],f["desc"]))
    for m in cf.methods:
        if only and m['name'] not in only: continue
        out.append('\n  method %s %s %s'%(flags(m["acc"]),m["name"],m["desc"]))
        c=parse_code(cf,m)
        if c and want_code:
            if c['lvt']: out.append('    locals: '+', '.join('%d=%s:%s'%(i,n,d) for i,n,d,_,_ in sorted(set(c['lvt']))))
            for l in disasm(cf,c['code']): out.append('    '+l)
            for s,e,h,t in c['exc']: out.append('    try[%d,%d) -> %d catch %s'%(s,e,h,t))
    return '\n'.join(out)

if __name__=='__main__':
    args=sys.argv[1:]
    want_code = '--sig' not in args
    only=None
    if '--only' in args:
        i=args.index('--only'); only=set(args[i+1].split(',')); del args[i:i+2]
    for p in args:
        if p.startswith('--'): continue
        print(dump(p, want_code, only))
