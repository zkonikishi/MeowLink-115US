package partywire

import (
	"encoding/binary"
	"testing"
)

// Synthetic boundary vectors, not native captures or sender supply.
func TestN7OpaquePreservationAndBoundaries(t *testing.T) {
	b := make([]byte, 49)
	binary.LittleEndian.PutUint16(b, 509)
	binary.LittleEndian.PutUint32(b[3:], 0x12345678)
	for i := 0; i < 37; i++ {
		b[7+i] = byte(i)
	}
	binary.LittleEndian.PutUint32(b[44:], 0x89abcdef)
	b[48] = 1
	v, e := DecodeN7Mode0(ProfileSHA256, b)
	if e != nil || v.Actor != 509 || v.UnknownHead != 0x12345678 || v.UnknownTail != 0x89abcdef || v.Flag != 1 {
		t.Fatal(v, e)
	}
	b[7] = 255
	if v.Opaque37[0] != 0 || v.Opaque37[36] != 36 {
		t.Fatal("mutable opaque alias")
	}
	for n := 0; n < 49; n++ {
		if _, e := DecodeN7Mode0(ProfileSHA256, b[:n]); e == nil {
			t.Fatalf("truncation %d", n)
		}
	}
	if _, e := DecodeN7Mode0(ProfileSHA256, append(b, 0)); e == nil {
		t.Fatal("guessed padding")
	}
	b[2] = 1
	if _, e := DecodeN7Mode0(ProfileSHA256, b); e == nil {
		t.Fatal("unknown mode")
	}
}
func TestN8UnknownValuesAndBoundaries(t *testing.T) {
	b := make([]byte, 11)
	binary.LittleEndian.PutUint16(b, 509)
	binary.LittleEndian.PutUint32(b[3:], 123)
	binary.LittleEndian.PutUint32(b[7:], 456)
	v, e := DecodeN8Mode0(ProfileSHA256, b)
	if e != nil || v.Actor != 509 || v.UnknownFirst != 123 || v.UnknownSecond != 456 {
		t.Fatal(v, e)
	}
	for n := 0; n < 11; n++ {
		if _, e := DecodeN8Mode0(ProfileSHA256, b[:n]); e == nil {
			t.Fatalf("truncation %d", n)
		}
	}
	if _, e := DecodeN8Mode0(ProfileSHA256, append(b, 0)); e == nil {
		t.Fatal("extra byte")
	}
	b[2] = 1
	if _, e := DecodeN8Mode0(ProfileSHA256, b); e == nil {
		t.Fatal("unknown mode")
	}
}
func TestProfileIsolation(t *testing.T) {
	for _, profile := range []string{"", "B823", "991ff5a67e32aace67b1a3f0b94ba67d22ada93501ec07cde4b15c448c3eb4bf"} {
		if _, e := DecodeN7Mode0(profile, make([]byte, 49)); e == nil {
			t.Fatal("foreign profile")
		}
		if _, e := DecodeN8Mode0(profile, make([]byte, 11)); e == nil {
			t.Fatal("foreign profile")
		}
	}
}
