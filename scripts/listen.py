"""Optional offline microphone recognition; JSON lines are consumed by Rust."""
import json
import queue
import sys
try:
    import sounddevice as sd
    from vosk import Model, KaldiRecognizer, SetLogLevel
    SetLogLevel(-1)
    model = Model(sys.argv[1])
    rate = int(sd.query_devices(kind='input')['default_samplerate'])
    recognizer = KaldiRecognizer(model, rate)
    audio = queue.Queue(maxsize=100)
    def callback(data, frames, time, status):
        try:
            audio.put_nowait(bytes(data))
        except queue.Full:
            pass
    with sd.RawInputStream(samplerate=rate, blocksize=4096, dtype='int16',
                           channels=1, callback=callback):
        while True:
            if recognizer.AcceptWaveform(audio.get()):
                print(recognizer.Result(), flush=True)
except Exception as error:
    print(json.dumps({'error': str(error)}), flush=True)
    sys.exit(1)
