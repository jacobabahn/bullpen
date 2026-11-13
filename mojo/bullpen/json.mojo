"""JSON: bullpen's task records and config.

Mojo's standard library has no JSON, so this is a small one of its own. A struct can't
hold a List of itself, so a document is an arena: every value is a Node in one List,
and an array's or object's children are indexes into it. Objects keep their keys in
file order, as Python's dicts do, so `dumps` gives back what `json.dumps(indent=2)`
would. Numbers keep the text they were read as: bullpen's files are written by
Python's json, whose text for a float is already its repr.
"""

comptime NULL = 0
comptime FALSE = 1
comptime TRUE = 2
comptime NUM = 3
comptime STR = 4
comptime ARR = 5
comptime OBJ = 6

comptime HEX = "0123456789abcdef"


struct Node(Copyable, Movable):
    var kind: Int
    var text: String  # a string's value, or a number as written
    var keys: List[String]  # an object's keys, in order
    var kids: List[Int]  # an array's items, or an object's values (by key order)

    def __init__(out self, kind: Int, var text: String = ""):
        self.kind = kind
        self.text = text^
        self.keys = List[String]()
        self.kids = List[Int]()


struct Doc(Copyable, Movable):
    """A JSON document: node 0 is its top value."""

    var nodes: List[Node]

    def __init__(out self):
        self.nodes = List[Node]()

    @staticmethod
    def parse(text: String) raises -> Doc:
        var doc = Doc()
        var p = _Parser(text)
        p.skip_ws()
        _ = p.value(doc)
        p.skip_ws()
        if p.i != p.n:
            raise Error("extra data in JSON")
        return doc^

    @staticmethod
    def read(path: String) raises -> Doc:
        with open(path, "r") as f:
            return Doc.parse(f.read())

    def add(mut self, var node: Node) -> Int:
        self.nodes.append(node^)
        return len(self.nodes) - 1

    # ---------- reading ----------

    def kind(self, n: Int) -> Int:
        return NULL if n < 0 else self.nodes[n].kind

    def get(self, n: Int, key: String) -> Int:
        """The index of object n's value for key, or -1 when it has none (or isn't an object)."""
        if n < 0 or self.nodes[n].kind != OBJ:
            return -1
        ref node = self.nodes[n]
        for i in range(len(node.keys)):
            if node.keys[i] == key:
                return node.kids[i]
        return -1

    def has(self, n: Int, key: String) -> Bool:
        return self.get(n, key) >= 0

    def str(self, n: Int, key: String) -> String:
        """Object n's string for key; "" when it's missing, null or not a string."""
        return self.text(self.get(n, key))

    def text(self, n: Int) -> String:
        if n < 0 or self.nodes[n].kind != STR:
            return ""
        return self.nodes[n].text

    def is_true(self, n: Int) -> Bool:
        """Python's truthiness of value n (-1, a missing one, is false)."""
        if n < 0:
            return False
        ref node = self.nodes[n]
        if node.kind == TRUE:
            return True
        if node.kind == NUM:
            return atof(node.text) != 0
        if node.kind == STR:
            return node.text.byte_length() > 0
        if node.kind == ARR or node.kind == OBJ:
            return len(node.kids) > 0
        return False

    def truthy(self, n: Int, key: String) -> Bool:
        return self.is_true(self.get(n, key))

    def int(self, n: Int, key: String, default: Int = 0) -> Int:
        var v = self.get(n, key)
        if v < 0 or self.nodes[v].kind != NUM:
            return default
        return Int(atof(self.nodes[v].text))

    def num(self, n: Int) -> Float64:
        if n < 0 or self.nodes[n].kind != NUM:
            return 0
        return atof(self.nodes[n].text)

    def len(self, n: Int) -> Int:
        return 0 if n < 0 else len(self.nodes[n].kids)

    def item(self, n: Int, i: Int) -> Int:
        return self.nodes[n].kids[i]

    def strs(self, n: Int, key: String) -> List[String]:
        """Object n's list of strings for key (empty when it has none)."""
        var out = List[String]()
        var a = self.get(n, key)
        if a >= 0 and self.nodes[a].kind == ARR:
            for k in self.nodes[a].kids:
                out.append(self.text(k))
        return out^

    def equal(self, a: Int, b: Int) -> Bool:
        """Python's == of two values in this document."""
        if a < 0 or b < 0:
            return a == b
        ref x = self.nodes[a]
        ref y = self.nodes[b]
        if x.kind == NUM and y.kind == NUM:
            return atof(x.text) == atof(y.text)
        if x.kind != y.kind or len(x.kids) != len(y.kids):
            return False
        if x.kind == STR:
            return x.text == y.text
        if x.kind == OBJ:
            for i in range(len(x.keys)):
                var j = self.get(b, x.keys[i])
                if j < 0 or not self.equal(x.kids[i], j):
                    return False
            return True
        for i in range(len(x.kids)):
            if not self.equal(x.kids[i], y.kids[i]):
                return False
        return True

    # ---------- changing ----------

    def put(mut self, n: Int, key: String, var value: Node) -> Int:
        """Set object n's key to a new node (in place when it has the key, else last)."""
        return self.link(n, key, self.add(value^))

    def link(mut self, n: Int, key: String, v: Int) -> Int:
        """Set object n's key to node v, already in this document."""
        ref node = self.nodes[n]
        for i in range(len(node.keys)):
            if node.keys[i] == key:
                node.kids[i] = v
                return v
        node.keys.append(key)
        node.kids.append(v)
        return v

    def put_str(mut self, n: Int, key: String, value: String) -> Int:
        return self.put(n, key, Node(STR, value))

    def put_null(mut self, n: Int, key: String) -> Int:
        return self.put(n, key, Node(NULL))

    def put_bool(mut self, n: Int, key: String, value: Bool) -> Int:
        return self.put(n, key, Node(TRUE if value else FALSE))

    def put_int(mut self, n: Int, key: String, value: Int) -> Int:
        return self.put(n, key, Node(NUM, String(value)))

    def pop(mut self, n: Int, key: String):
        """Drop object n's key, if it has it."""
        ref node = self.nodes[n]
        for i in range(len(node.keys)):
            if node.keys[i] == key:
                _ = node.keys.pop(i)
                _ = node.kids.pop(i)
                return

    # ---------- writing ----------

    def dumps(self, n: Int = 0, indent: Int = 2, depth: Int = 0) -> String:
        """Value n as Python's json.dumps(value, indent=indent) writes it (ensure_ascii),
        as if nested depth levels deep."""
        var out = String()
        self._dump(n, indent, depth, out)
        return out^

    def _dump(self, n: Int, indent: Int, depth: Int, mut out: String):
        ref node = self.nodes[n]
        if node.kind == NULL:
            out += "null"
        elif node.kind == TRUE:
            out += "true"
        elif node.kind == FALSE:
            out += "false"
        elif node.kind == NUM:
            out += node.text
        elif node.kind == STR:
            quote(node.text, out)
        elif len(node.kids) == 0:
            out += "[]" if node.kind == ARR else "{}"
        else:
            var inner = "\n" + " " * (indent * (depth + 1))
            out += "[" if node.kind == ARR else "{"
            for i in range(len(node.kids)):
                if i:
                    out += ","
                out += inner
                if node.kind == OBJ:
                    quote(node.keys[i], out)
                    out += ": "
                self._dump(node.kids[i], indent, depth + 1, out)
            out += "\n" + " " * (indent * depth)
            out += "]" if node.kind == ARR else "}"


def copy_into(mut dst: Doc, src: Doc, n: Int) -> Int:
    """A deep copy of src's value n, added to dst: its index there."""
    ref node = src.nodes[n]
    var out = dst.add(Node(node.kind, node.text))
    for i in range(len(node.kids)):
        var k = copy_into(dst, src, node.kids[i])
        if node.kind == OBJ:
            dst.nodes[out].keys.append(node.keys[i])
        dst.nodes[out].kids.append(k)
    return out


def dumps_list(docs: List[Doc], indent: Int = 2) -> String:
    """A list of documents' top values, as json.dumps(list, indent=indent) writes it."""
    if len(docs) == 0:
        return "[]"
    var out = String("[")
    for i in range(len(docs)):
        out += ("," if i else "") + "\n" + " " * indent + docs[i].dumps(0, indent, 1)
    return out + "\n]"


def quote(s: String, mut out: String):
    """s as a JSON string, escaped as Python's json does with ensure_ascii."""
    out += '"'
    for cp in s.codepoints():
        var c = Int(cp)
        if c == 0x22:
            out += '\\"'
        elif c == 0x5C:
            out += "\\\\"
        elif c == 0x0A:
            out += "\\n"
        elif c == 0x0D:
            out += "\\r"
        elif c == 0x09:
            out += "\\t"
        elif c == 0x08:
            out += "\\b"
        elif c == 0x0C:
            out += "\\f"
        elif c < 0x20 or c > 0x7E:
            if c > 0xFFFF:
                var v = c - 0x10000
                _hex4(0xD800 + (v >> 10), out)
                _hex4(0xDC00 + (v & 0x3FF), out)
            else:
                _hex4(c, out)
        else:
            out += String(cp)
    out += '"'


def _hex4(c: Int, mut out: String):
    out += "\\u"
    for shift in [12, 8, 4, 0]:
        out += String(HEX[byte=(c >> shift) & 0xF])


def atof(s: String) -> Float64:
    try:
        return Float64(s)
    except:
        return 0


struct _Parser:
    var src: String
    var i: Int
    var n: Int

    def __init__(out self, src: String):
        self.src = src
        self.i = 0
        self.n = src.byte_length()

    def at(self) -> Int:
        return Int(self.src.as_bytes()[self.i])

    def skip_ws(mut self):
        while self.i < self.n:
            var c = self.at()
            if c != 32 and c != 10 and c != 13 and c != 9:
                return
            self.i += 1

    def expect(mut self, word: String) raises:
        if self.src[byte=self.i:self.i + word.byte_length()] != word:
            raise Error("bad JSON at byte " + String(self.i))
        self.i += word.byte_length()

    def value(mut self, mut doc: Doc) raises -> Int:
        self.skip_ws()
        if self.i >= self.n:
            raise Error("JSON ended early")
        var c = self.at()
        if c == ord("{"):
            return self.object(doc)
        if c == ord("["):
            return self.array(doc)
        if c == ord('"'):
            return doc.add(Node(STR, self.string()))
        if c == ord("t"):
            self.expect("true")
            return doc.add(Node(TRUE))
        if c == ord("f"):
            self.expect("false")
            return doc.add(Node(FALSE))
        if c == ord("n"):
            self.expect("null")
            return doc.add(Node(NULL))
        var start = self.i
        while self.i < self.n:
            var d = self.at()
            if not ((d >= ord("0") and d <= ord("9")) or d == ord("-") or d == ord("+")
                    or d == ord(".") or d == ord("e") or d == ord("E")):
                break
            self.i += 1
        if self.i == start:
            raise Error("bad JSON at byte " + String(start))
        return doc.add(Node(NUM, String(self.src[byte=start:self.i])))

    def object(mut self, mut doc: Doc) raises -> Int:
        var n = doc.add(Node(OBJ))
        self.i += 1
        self.skip_ws()
        if self.at() == ord("}"):
            self.i += 1
            return n
        while True:
            self.skip_ws()
            if self.at() != ord('"'):
                raise Error("bad JSON key at byte " + String(self.i))
            var key = self.string()
            self.skip_ws()
            self.expect(":")
            var v = self.value(doc)
            doc.nodes[n].keys.append(key^)
            doc.nodes[n].kids.append(v)
            self.skip_ws()
            if self.at() == ord(","):
                self.i += 1
                continue
            self.expect("}")
            return n

    def array(mut self, mut doc: Doc) raises -> Int:
        var n = doc.add(Node(ARR))
        self.i += 1
        self.skip_ws()
        if self.at() == ord("]"):
            self.i += 1
            return n
        while True:
            var v = self.value(doc)
            doc.nodes[n].kids.append(v)
            self.skip_ws()
            if self.at() == ord(","):
                self.i += 1
                continue
            self.expect("]")
            return n

    def string(mut self) raises -> String:
        self.i += 1
        var out = String()
        var start = self.i
        while True:
            if self.i >= self.n:
                raise Error("unterminated JSON string")
            var c = self.at()
            if c == ord('"'):
                out += self.src[byte=start:self.i]
                self.i += 1
                return out^
            if c != ord("\\"):
                self.i += 1
                continue
            out += self.src[byte=start:self.i]
            var e = Int(self.src.as_bytes()[self.i + 1])
            self.i += 2
            if e == ord("n"):
                out += "\n"
            elif e == ord("t"):
                out += "\t"
            elif e == ord("r"):
                out += "\r"
            elif e == ord("b"):
                out += "\x08"
            elif e == ord("f"):
                out += "\x0c"
            elif e == ord("u"):
                var cp = self.hex4()
                if cp >= 0xD800 and cp < 0xDC00 and self.src[byte=self.i:self.i + 2] == "\\u":
                    self.i += 2
                    var lo = self.hex4()
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
                out += String(Codepoint(unsafe_unchecked_codepoint=UInt32(cp)))
            else:
                out += String(Codepoint(UInt8(e)))
            start = self.i

    def hex4(mut self) raises -> Int:
        var v = 0
        for _ in range(4):
            var c = self.at()
            var d = c - ord("0") if c <= ord("9") else (c | 0x20) - ord("a") + 10
            v = v * 16 + d
            self.i += 1
        return v
