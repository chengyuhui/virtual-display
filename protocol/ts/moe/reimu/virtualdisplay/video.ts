// automatically generated by the FlatBuffers compiler, do not modify

import * as flatbuffers from 'flatbuffers';

export class Video {
  bb: flatbuffers.ByteBuffer|null = null;
  bb_pos = 0;
  __init(i:number, bb:flatbuffers.ByteBuffer):Video {
  this.bb_pos = i;
  this.bb = bb;
  return this;
}

static getRootAsVideo(bb:flatbuffers.ByteBuffer, obj?:Video):Video {
  return (obj || new Video()).__init(bb.readInt32(bb.position()) + bb.position(), bb);
}

static getSizePrefixedRootAsVideo(bb:flatbuffers.ByteBuffer, obj?:Video):Video {
  bb.setPosition(bb.position() + flatbuffers.SIZE_PREFIX_LENGTH);
  return (obj || new Video()).__init(bb.readInt32(bb.position()) + bb.position(), bb);
}

timestamp():bigint {
  const offset = this.bb!.__offset(this.bb_pos, 4);
  return offset ? this.bb!.readUint64(this.bb_pos + offset) : BigInt('0');
}

static startVideo(builder:flatbuffers.Builder) {
  builder.startObject(1);
}

static addTimestamp(builder:flatbuffers.Builder, timestamp:bigint) {
  builder.addFieldInt64(0, timestamp, BigInt('0'));
}

static endVideo(builder:flatbuffers.Builder):flatbuffers.Offset {
  const offset = builder.endObject();
  return offset;
}

static createVideo(builder:flatbuffers.Builder, timestamp:bigint):flatbuffers.Offset {
  Video.startVideo(builder);
  Video.addTimestamp(builder, timestamp);
  return Video.endVideo(builder);
}
}
