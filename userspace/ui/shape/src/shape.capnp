@0xd0a1b2c3e4f5a6b7;

struct ShapeRequest {
    text @0 :Text;
    pixelSize @1 :UInt16;
    script @2 :Text;
    direction @3 :Direction;
}

enum Direction {
    ltr @0;
    rtl @1;
}

struct ShapeResponse {
    glyphRun @0 :GlyphRun;
}

struct GlyphRun {
    glyphs @0 :List(Glyph);
    clusterMap @1 :List(UInt32);
    width @2 :UInt32;
    height @3 :UInt32;
}

struct Glyph {
    index @0 :UInt32;
    x @1 :Int32;
    y @2 :Int32;
    advance @3 :Int32;
    fontId @4 :UInt32;
}
