package wre

import (
	"encoding/binary"
	"encoding/json"
	"io"
)

const (
	protocolVersion = 1
	frameHeaderLen  = 8
	maxJSONBytes    = 64 * 1024 * 1024
	maxBinBytes     = 512 * 1024 * 1024
)

type frame struct {
	json []byte
	bin  []byte
}

func writeFrame(w io.Writer, f frame) error {
	if len(f.json) > maxJSONBytes {
		return newError(KindProtocol, "frame json length %d exceeds the 64 MiB cap", len(f.json))
	}
	if len(f.bin) > maxBinBytes {
		return newError(KindProtocol, "frame binary length %d exceeds the 512 MiB cap", len(f.bin))
	}
	buf := make([]byte, frameHeaderLen+len(f.json)+len(f.bin))
	binary.BigEndian.PutUint32(buf[0:4], uint32(len(f.json)))
	binary.BigEndian.PutUint32(buf[4:8], uint32(len(f.bin)))
	copy(buf[frameHeaderLen:], f.json)
	copy(buf[frameHeaderLen+len(f.json):], f.bin)
	_, err := w.Write(buf)
	return err
}

func readFrame(r io.Reader) (frame, error) {
	var header [frameHeaderLen]byte
	if _, err := io.ReadFull(r, header[:]); err != nil {
		if err == io.EOF {
			return frame{}, io.EOF
		}
		return frame{}, newError(KindProtocol, "truncated frame header: %v", err)
	}

	jsonLen := binary.BigEndian.Uint32(header[0:4])
	binLen := binary.BigEndian.Uint32(header[4:8])
	if jsonLen > maxJSONBytes {
		return frame{}, newError(KindProtocol, "frame json length %d exceeds the 64 MiB cap", jsonLen)
	}
	if binLen > maxBinBytes {
		return frame{}, newError(KindProtocol, "frame binary length %d exceeds the 512 MiB cap", binLen)
	}

	jsonBuf := make([]byte, jsonLen)
	if _, err := io.ReadFull(r, jsonBuf); err != nil {
		return frame{}, newError(KindProtocol, "truncated frame json: %v", err)
	}

	binBuf := make([]byte, binLen)
	if binLen > 0 {
		if _, err := io.ReadFull(r, binBuf); err != nil {
			return frame{}, newError(KindProtocol, "truncated frame binary: %v", err)
		}
	}

	return frame{json: jsonBuf, bin: binBuf}, nil
}

type reqEnvelope struct {
	T          string          `json:"t"`
	V          int             `json:"v"`
	ID         uint64          `json:"id"`
	Op         string          `json:"op"`
	Session    string          `json:"session,omitempty"`
	Params     json.RawMessage `json:"params,omitempty"`
	DeadlineMs int64           `json:"deadline_ms,omitempty"`
}

type resEnvelope struct {
	T      string          `json:"t"`
	V      int             `json:"v"`
	ID     uint64          `json:"id"`
	OK     bool            `json:"ok"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  json.RawMessage `json:"error,omitempty"`
	TookMs int64           `json:"took_ms,omitempty"`
}

type evtEnvelope struct {
	T     string          `json:"t"`
	V     int             `json:"v"`
	ID    uint64          `json:"id"`
	Event string          `json:"event"`
	Data  json.RawMessage `json:"data,omitempty"`
}

type cancelEnvelope struct {
	T  string `json:"t"`
	V  int    `json:"v"`
	ID uint64 `json:"id"`
}
