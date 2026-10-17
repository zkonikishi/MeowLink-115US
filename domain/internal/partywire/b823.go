// Package partywire decodes evidenced B823 logical bodies for offline review.
// It provides no encoder, native success projection, or authorization decision.
package partywire

import (
	"encoding/binary"
	"fmt"
)

const ProfileSHA256 = "b8235efe5dbf140ea4c535a026e137c4b34cfaecd7c97c78ed5997c377327c95"

type N7Mode0 struct {
	Actor       uint16
	UnknownHead uint32
	Opaque37    [37]byte
	UnknownTail uint32
	Flag        byte
}
type N8Mode0 struct {
	Actor                       uint16
	UnknownFirst, UnknownSecond uint32
}

// DecodeN7Mode0 accepts an already separated logical body. Transport padding
// must be handled by a separately proven codec, never inferred here.
func DecodeN7Mode0(profile string, body []byte) (N7Mode0, error) {
	var v N7Mode0
	if profile != ProfileSHA256 || len(body) != 49 {
		return v, fmt.Errorf("unsupported N7 profile or logical length")
	}
	if body[2] != 0 {
		return v, fmt.Errorf("unsupported N7 mode")
	}
	v.Actor = binary.LittleEndian.Uint16(body)
	v.UnknownHead = binary.LittleEndian.Uint32(body[3:7])
	copy(v.Opaque37[:], body[7:44])
	v.UnknownTail = binary.LittleEndian.Uint32(body[44:48])
	v.Flag = body[48]
	return v, nil
}
func DecodeN8Mode0(profile string, body []byte) (N8Mode0, error) {
	var v N8Mode0
	if profile != ProfileSHA256 || len(body) != 11 {
		return v, fmt.Errorf("unsupported N8 profile or logical length")
	}
	if body[2] != 0 {
		return v, fmt.Errorf("unsupported N8 mode")
	}
	v.Actor = binary.LittleEndian.Uint16(body)
	v.UnknownFirst = binary.LittleEndian.Uint32(body[3:7])
	v.UnknownSecond = binary.LittleEndian.Uint32(body[7:11])
	return v, nil
}
