<?php
// The messages between the orchestrator and a worker: one line each, a
// serialized value in base64, on the descriptors the worker is started with
// (3 to read, 4 to write; its standard streams are /dev/null).

/** A message on a stream: one line. */
function send(mixed $stream, mixed $message): void
{
    $line = base64_encode(serialize($message)) . "\n";
    if (@fwrite($stream, $line) !== strlen($line)) {
        throw new RuntimeException('the other end of the pipe is gone');
    }
    fflush($stream);
}

/** The message on a stream, or `null` when it was closed. */
function receive(mixed $stream): mixed
{
    $line = fgets($stream);
    if ($line === false) {
        return null;
    }
    return unserialize((string) base64_decode(trim($line)), ['allowed_classes' => false]);
}

/** The worker's end of the protocol. */
final class Protocol
{
    /** @var resource */
    public static $in;
    /** @var resource */
    public static $out;

    /**
     * Takes descriptors 3 and 4 as streams. `php://fd` copies a descriptor,
     * so the originals are closed, or every child would inherit them.
     */
    public static function open(): void
    {
        $in = fopen('php://fd/3', 'rb');
        $out = fopen('php://fd/4', 'wb');
        if ($in === false || $out === false) {
            exit(1);
        }
        self::$in = $in;
        self::$out = $out;
        if (class_exists('FFI')) {
            try {
                $libc = FFI::cdef('int close(int fd);', 'libc.so.6');
                $libc->close(3);
                $libc->close(4);
            } catch (Throwable) {
            }
        }
    }

    /** In a child: the protocol is not the child's to write to. */
    public static function release(): void
    {
        fclose(self::$in);
        fclose(self::$out);
    }
}

/**
 * What a worker runs, for the orchestrator to compare with the others and for
 * the sidecar: each extension with its version, a hash of its info text (none
 * for Core and FFI, whose text differs between runs) and the functions and
 * classes it provides.
 *
 * @return array{php: string, extensions: array<string, array{version: ?string, info: ?string, provides: list<string>}>, ffi: bool}
 */
function manifest(): array
{
    $names = get_loaded_extensions();
    sort($names);
    $extensions = [];
    foreach ($names as $name) {
        $reflection = new ReflectionExtension($name);
        ob_start();
        $reflection->info();
        $text = (string) ob_get_clean();
        $provides = array_keys($reflection->getFunctions());
        foreach ($reflection->getClasses() as $class) {
            $provides[] = $class->getName();
        }
        sort($provides);
        $version = phpversion($name);
        $extensions[$name] = [
            'version' => $version === false ? null : $version,
            'info' => in_array($name, ['Core', 'FFI'], true) ? null : sha1($text),
            'provides' => $provides,
        ];
    }
    /** @var array{php: string, extensions: array<string, array{version: ?string, info: ?string, provides: list<string>}>, ffi: bool} $manifest */
    $manifest = ['php' => PHP_VERSION, 'extensions' => $extensions, 'ffi' => class_exists('FFI')];
    return $manifest;
}
