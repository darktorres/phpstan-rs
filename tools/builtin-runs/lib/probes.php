<?php
// The values a parameter is probed with, and the lines a probe's call yields.

require_once __DIR__ . '/stubdocs.php';

/**
 * Whether the stubs document an untyped parameter as a resource. PHP's
 * reflection shows no type for it, so the stubs' docblocks say what it takes.
 */
function documented_resource(ReflectionParameter $param): bool
{
    static $map = null;
    $map ??= untyped_resource_params(phpstorm_stubs_dir());
    $function = $param->getDeclaringFunction();
    $key = $function instanceof ReflectionMethod
        ? strtolower($function->getDeclaringClass()->getName() . '::' . $function->getName())
        : strtolower($function->getName());
    return isset($map[$key][$param->getName()]);
}

/** Whether `$type` admits a value of the named type. */
function admits(?ReflectionType $type, string $name): bool
{
    if ($type === null) {
        return true;
    }
    if ($type instanceof ReflectionNamedType) {
        $t = strtolower($type->getName());
        return $t === $name || $t === 'mixed'
            || ($name === 'object' && !in_array($t, ['int', 'float', 'string', 'bool', 'array', 'null', 'false', 'true', 'void', 'never', 'callable', 'iterable', 'resource'], true))
            || ($name === 'iterable' && in_array($t, ['iterable', 'traversable', 'iterator', 'iteratoraggregate'], true));
    }
    foreach ($type->getTypes() as $member) {
        if (admits($member, $name)) {
            return true;
        }
    }
    return false;
}

/**
 * Whether `$type` admits a value of the class `$class`: `mixed` and no type do,
 * `object` does, a type the class is an instance of does (an interface it
 * implements, or its own class), `iterable` does for a `Traversable`, and a
 * union with `string` does for an object with `__toString` (the recording object).
 */
function accepts_value(?ReflectionType $type, string $class): bool
{
    if ($type === null) {
        return true;
    }
    if ($type instanceof ReflectionNamedType) {
        $t = strtolower($type->getName());
        return $t === 'mixed' || $t === 'object'
            || ($t === 'iterable' && is_a($class, Traversable::class, true))
            || is_a($class, $type->getName(), true);
    }
    // In a union with `string`, an object the class of which is not a member is converted
    // to a string by its `__toString` (PHP's coercion, outside strict types): the recording
    // object has one, so it reaches that user code.
    $members = $type->getTypes();
    $hasString = array_filter($members, static fn (ReflectionType $member): bool => $member instanceof ReflectionNamedType && strtolower($member->getName()) === 'string') !== [];
    if ($hasString && $class === RecordingObject::class) {
        return true;
    }
    foreach ($members as $member) {
        if (accepts_value($member, $class)) {
            return true;
        }
    }
    return false;
}

/** Whether `$type` names a callable, not merely admits one as `mixed` does. */
function names_callable(?ReflectionType $type): bool
{
    if ($type instanceof ReflectionNamedType) {
        return in_array(strtolower($type->getName()), ['callable', 'closure'], true);
    }
    if ($type instanceof ReflectionUnionType) {
        foreach ($type->getTypes() as $member) {
            if (names_callable($member)) {
                return true;
            }
        }
    }
    return false;
}

/**
 * The probes for a parameter: each a description and a function that makes
 * the value passed.
 *
 * @return list<array{string, Closure(): mixed}>
 */
function probes_for(ReflectionParameter $param): array
{
    $type = $param->getType();
    // A parameter the stubs document as a resource takes a stream or a stream
    // context, and nothing else: every other probe is a TypeError before any user
    // code. The context's notification callback is user code.
    if (documented_resource($param)) {
        return [['stream', static fn () => recording_stream()], ['context', static fn () => recording_context()]];
    }
    $probes = [];
    if (admits($type, 'callable') || admits($type, 'closure')) {
        $probes[] = ['callable', static fn () => recording_callable()];
    }
    if (accepts_value($type, RecordingObject::class)) {
        $probes[] = ['object', static fn () => new RecordingObject()];
    }
    if (accepts_value($type, RecordingIterator::class)) {
        $probes[] = ['iterator', static fn () => new RecordingIterator()];
    }
    if (accepts_value($type, RecordingSleeper::class)) {
        $probes[] = ['sleeper', static fn () => new RecordingSleeper()];
    }
    if (admits($type, 'string')) {
        $probes[] = ['url', static fn () => 'rec://probe'];
    }
    if (admits($type, 'array')) {
        $probes[] = ['objects', static fn () => [new RecordingObject(), new RecordingObject()]];
        $probes[] = ['callables', static fn () => [recording_callable(), recording_callable()]];
        $probes[] = ['urls', static fn () => ['rec://probe']];
    }
    if (admits($type, 'resource') || $type === null || (string) $type === 'mixed') {
        $probes[] = ['stream', static fn () => recording_stream()];
    }
    return $probes;
}

/**
 * A valid value of a class or resource type that no plain value stands for,
 * made fresh by the child that calls with it: a stream, an XML writer, an
 * Intl formatter. Keyed by the lowercased name of the type. Only the types a
 * plain value cannot satisfy are here; each value needs no server, file or
 * network. A union has no entry: its scalar members are plain values.
 */
function valid_value(?ReflectionType $type): ?Closure
{
    static $values = null;
    if (!$type instanceof ReflectionNamedType) {
        return null;
    }
    $values ??= [
        'xmlwriter' => static fn (): XMLWriter => xmlwriter_open_memory(),
        'xmlparser' => static fn (): XMLParser => xml_parser_create(),
        // Not connected: a call on it fails for want of a connection, which the table records as a gap.
        'mysqli' => static fn (): mysqli => new mysqli(),
        'socket' => static fn (): Socket => socket_create(AF_UNIX, SOCK_STREAM, 0),
        // Made in the worker's own IPC namespace, which the sandbox gives it; without one the queue is the host's.
        // One message is queued, so a receive returns instead of waiting on the empty queue.
        'sysvmessagequeue' => static function (): SysvMessageQueue {
            $queue = msg_get_queue(0x5150);
            msg_send($queue, 1, 'x');
            return $queue;
        },
        'datetimeinterface' => static fn (): DateTimeImmutable => new DateTimeImmutable('2000-01-01 00:00:00', new DateTimeZone('UTC')),
        'datetime' => static fn (): DateTime => new DateTime('2000-01-01 00:00:00', new DateTimeZone('UTC')),
        'dateinterval' => static fn (): DateInterval => new DateInterval('P1D'),
        'gdimage' => static fn (): mixed => imagecreatetruecolor(1, 1),
        'finfo' => static fn (): finfo => new finfo(FILEINFO_MIME_TYPE),
        'hashcontext' => static fn (): HashContext => hash_init('md5'),
        'curlhandle' => static fn (): CurlHandle => curl_init(),
        'curlmultihandle' => static fn (): CurlMultiHandle => curl_multi_init(),
        'numberformatter' => static fn (): NumberFormatter => numfmt_create('en', NumberFormatter::DECIMAL),
        'intldateformatter' => static fn (): IntlDateFormatter => datefmt_create('en', IntlDateFormatter::FULL, IntlDateFormatter::FULL, 'UTC'),
        'collator' => static fn (): Collator => collator_create('en'),
        'intltimezone' => static fn (): IntlTimeZone => intltz_create_default(),
        'intlcalendar' => static fn (): IntlCalendar => intlcal_create_instance('UTC', 'en'),
        'messageformatter' => static fn (): MessageFormatter => msgfmt_create('en', 'x'),
        'deflatecontext' => static fn (): DeflateContext => deflate_init(ZLIB_ENCODING_RAW),
        'inflatecontext' => static fn (): InflateContext => inflate_init(ZLIB_ENCODING_RAW),
        // With an image: most of Imagick's methods throw on an empty object.
        'imagick' => static function (): Imagick {
            $image = new Imagick();
            $image->newImage(1, 1, new ImagickPixel('white'));
            return $image;
        },
        'imagickdraw' => static fn (): ImagickDraw => new ImagickDraw(),
        'imagickpixel' => static fn (): ImagickPixel => new ImagickPixel('white'),
        // Valid for every function that takes the type, so they are type defaults.
        'iterator' => static fn (): ArrayIterator => new ArrayIterator([1, 2]),
        'datetimezone' => static fn (): DateTimeZone => new DateTimeZone('UTC'),
        'resourcebundle' => static fn (): ResourceBundle => resourcebundle_create('en', null),
        'curlsharehandle' => static fn (): CurlShareHandle => curl_share_init(),
        'opensslasymmetrickey' => static fn (): OpenSSLAsymmetricKey => openssl_pkey_new(['private_key_bits' => 1024, 'private_key_type' => OPENSSL_KEYTYPE_RSA]),
        'streambucket' => static fn (): StreamBucket => stream_bucket_new(fopen('php://memory', 'r+'), 'x'),
        'domdocument' => static function (): DOMDocument {
            $document = new DOMDocument();
            $document->loadXML('<a/>');
            return $document;
        },
        // In the worker's IPC namespace, as the message queue is.
        'shmop' => static fn (): Shmop => shmop_open(0x5150, 'c', 0644, 64),
        'sysvsharedmemory' => static fn (): SysvSharedMemory => shm_attach(0x5151, 64),
    ];
    return $values[strtolower($type->getName())] ?? null;
}

/** A stream context whose notification callback is a recording callable. */
function recording_context(): mixed
{
    $context = stream_context_create();
    stream_context_set_params($context, ['notification' => recording_callable()]);
    return $context;
}

/** A stream holding a 1x1 PNG, for the functions that read an image from a file handle. */
function png_stream_sample(): mixed
{
    $image = imagecreatetruecolor(1, 1);
    ob_start();
    imagepng($image);
    $png = ob_get_clean();
    $stream = fopen('php://memory', 'r+');
    fwrite($stream, $png);
    rewind($stream);
    return $stream;
}

/** A directory of a zip archive with one entry, made in the worker's temporary directory. */
function zip_dir_sample(): mixed
{
    $path = tempnam(sys_get_temp_dir(), 'zip');
    $archive = new ZipArchive();
    $archive->open($path, ZipArchive::CREATE);
    $archive->addFromString('a', 'x');
    $archive->close();
    return zip_open($path);
}

/** An entry of the zip archive `zip_dir_sample` makes. */
function zip_entry_sample(): mixed
{
    return zip_read(zip_dir_sample());
}

/**
 * A valid value for one parameter of one function, where its type is not
 * enough: a string the function accepts only from a set of names. Keyed by the
 * lowercased function or `class::method`, then by position.
 */
function valid_site(ReflectionParameter $param): ?Closure
{
    // Each value is a closure, so a constant of an extension that is not loaded is read only when its function is probed.
    static $sites = [
        'mb_convert_variables' => [
            0 => 'UTF-8',
            1 => 'UTF-8',
        ],
        'curl_setopt' => [1 => static fn (): int => CURLOPT_FOLLOWLOCATION],
        'curl_multi_setopt' => [1 => static fn (): int => CURLMOPT_MAXCONNECTS],
        'curl_share_setopt' => [1 => static fn (): int => CURLSHOPT_SHARE],
        'datefmt_create' => [0 => 'en'],
        'datefmt_format_object' => [0 => static fn (): DateTime => new DateTime('2000-01-01 00:00:00', new DateTimeZone('UTC'))],
        'deflate_init' => [0 => static fn (): int => ZLIB_ENCODING_RAW],
        'domdocument::registernodeclass' => [0 => 'DOMElement', 1 => static fn (): null => null],
        'imageaffinematrixget' => [0 => static fn (): int => IMG_AFFINE_TRANSLATE, 1 => static fn (): array => ['x' => 1, 'y' => 1]],
        'imagecolormatch' => [1 => static function (): GdImage {
            $palette = imagecreate(1, 1);
            imagecolorallocate($palette, 255, 255, 255);
            return $palette;
        }],
        'imageaffine' => [1 => static fn (): array => [1, 0, 0, 1, 0, 0]],
        'imageconvolution' => [1 => static fn (): array => [[1, 1, 1], [1, 1, 1], [1, 1, 1]]],
        'imagecrop' => [1 => static fn (): array => ['x' => 0, 'y' => 0, 'width' => 1, 'height' => 1]],
        'imagefilledpolygon' => [1 => static fn (): array => [0, 0, 1, 0, 0, 1], 2 => 3],
        'imageopenpolygon' => [1 => static fn (): array => [0, 0, 1, 0, 0, 1], 2 => 3],
        'curl_setopt_array' => [1 => static fn (): array => [CURLOPT_FOLLOWLOCATION => true]],
        'datefmt_set_timezone' => [1 => 'UTC'],
        'intlcal_set_time_zone' => [1 => 'UTC'],
        'xsltprocessor::transformtouri' => [0 => static function (): DOMDocument {
            $document = new DOMDocument();
            $document->loadXML('<a/>');
            return $document;
        }],
        'settype' => [1 => 'int'],
        'simplexml_load_file' => [1 => static fn (): null => null],
        'simplexml_load_string' => [1 => static fn (): null => null],
        'spldoublylinkedlist::add' => [0 => 0],
        'splobjectstorage::offsetset' => [0 => static fn (): stdClass => new stdClass()],
        'weakmap::offsetset' => [0 => static fn (): stdClass => new stdClass()],
        'imagick::readimagefile' => [0 => static fn (): mixed => png_stream_sample()],
        'imagick::pingimagefile' => [0 => static fn (): mixed => png_stream_sample()],
        'copy' => [2 => static fn (): mixed => stream_context_create()],
        'dir' => [1 => static fn (): mixed => stream_context_create()],
        'file' => [2 => static fn (): mixed => stream_context_create()],
        'file_get_contents' => [2 => static fn (): mixed => stream_context_create()],
        'file_put_contents' => [3 => static fn (): mixed => stream_context_create()],
        'finfo_file' => [3 => static fn (): mixed => stream_context_create()],
        'finfo::file' => [2 => static fn (): mixed => stream_context_create()],
        'fopen' => [3 => static fn (): mixed => stream_context_create()],
        'get_headers' => [2 => static fn (): mixed => stream_context_create()],
        'hash_update_file' => [2 => static fn (): mixed => stream_context_create()],
        'libxml_set_streams_context' => [0 => static fn (): mixed => stream_context_create()],
        'mkdir' => [3 => static fn (): mixed => stream_context_create()],
        'opendir' => [1 => static fn (): mixed => stream_context_create()],
        'readfile' => [2 => static fn (): mixed => stream_context_create()],
        'rename' => [2 => static fn (): mixed => stream_context_create()],
        'rmdir' => [1 => static fn (): mixed => stream_context_create()],
        'scandir' => [2 => static fn (): mixed => stream_context_create()],
        'stream_socket_client' => [5 => static fn (): mixed => stream_context_create()],
        'stream_socket_server' => [4 => static fn (): mixed => stream_context_create()],
        'unlink' => [1 => static fn (): mixed => stream_context_create()],
        'stream_context_set_options' => [0 => static fn (): mixed => stream_context_create(), 1 => static fn (): array => ['http' => ['method' => 'GET']]],
        'mb_ereg_replace_callback' => [3 => static fn (): null => null],
        'strtr' => [1 => static fn (): array => ['a' => 'b']],
        // A no-op command and no descriptors: proc_open starts a process whatever it is given.
        'proc_open' => [0 => 'true', 1 => static fn (): array => []],
        'zip_entry_close' => [0 => static fn (): mixed => zip_entry_sample()],
        'zip_entry_compressedsize' => [0 => static fn (): mixed => zip_entry_sample()],
        'zip_entry_compressionmethod' => [0 => static fn (): mixed => zip_entry_sample()],
        'zip_entry_filesize' => [0 => static fn (): mixed => zip_entry_sample()],
        'zip_entry_name' => [0 => static fn (): mixed => zip_entry_sample()],
        'zip_entry_open' => [0 => static fn (): mixed => zip_dir_sample(), 1 => static fn (): mixed => zip_entry_sample()],
        'zip_entry_read' => [0 => static fn (): mixed => zip_entry_sample()],
        'zip_read' => [0 => static fn (): mixed => zip_dir_sample()],
        'zip_close' => [0 => static fn (): mixed => zip_dir_sample()],
        'stream_context_set_option' => [1 => 'http', 2 => 'method', 3 => 'GET'],
    ];
    $function = $param->getDeclaringFunction();
    $key = $function instanceof ReflectionMethod
        ? strtolower($function->getDeclaringClass()->getName() . '::' . $function->getName())
        : strtolower($function->getName());
    $value = $sites[$key][$param->getPosition()] ?? null;
    if ($value === null) {
        return null;
    }
    return $value instanceof Closure ? $value : static fn (): mixed => $value;
}

/** A plain value for a parameter a probe is not at. */
function plain_value(ReflectionParameter $param): mixed
{
    $site = valid_site($param);
    if ($site !== null) {
        return $site();
    }
    $type = $param->getType();
    // A memory stream serves every resource kind the stubs name: PHP takes a
    // stream where it takes a stream context, and a function that needs another
    // kind throws, which the table records. Untyped and `mixed` parameters are
    // the ones the stubs document as resources (Imagick's take `mixed`).
    if (($type === null || (string) $type === 'mixed') && documented_resource($param)) {
        return fopen('php://memory', 'r+');
    }
    if ($type === null) {
        return 1;
    }
    if ($type->allowsNull() && !admits($type, 'int') && !admits($type, 'string')) {
        return null;
    }
    $valid = valid_value($type);
    if ($valid !== null) {
        return $valid();
    }
    foreach (['int' => 1, 'string' => 'x', 'float' => 1.0, 'bool' => false, 'array' => [1, 2]] as $name => $value) {
        if (admits($type, $name)) {
            return $value;
        }
    }
    if (admits($type, 'callable')) {
        return static function (mixed ...$args): int {
            Seen::$given[] = $args;
            return 0;
        };
    }
    if (admits($type, 'iterable')) {
        return [1, 2];
    }
    if (admits($type, 'object')) {
        return new stdClass();
    }
    return null;
}

/**
 * The arguments for a call with the probe value at `$at`: the parameters
 * before it plain, the ones after it left out when they may be.
 *
 * @param list<ReflectionParameter> $params
 * @return list<mixed>
 */
function arguments(array $params, int $at, mixed $probe): array
{
    $args = [];
    foreach ($params as $i => $param) {
        if ($i > $at && $param->isOptional()) {
            break;
        }
        $args[$i] = $i === $at ? $probe : plain_value($param);
    }
    return $args;
}

/** Where a probe is: its parameter's position, and `[]` for the elements of an array. */
function where(string $prefix, int $at, string $probe): string
{
    return $prefix . $at . (in_array($probe, ['objects', 'callables', 'urls'], true) ? '[]' : '');
}

/** The lines for one probe's call: `probed` once the call was reached. */
function record(string $builtin, string $where, ?array $seen, array &$out): void
{
    Stats::child($builtin, $where);
    if ($seen === null) {
        Stats::failed($builtin, $where);
        return;
    }
    foreach (array_keys($seen) as $entry) {
        [$seenWhere, $kind, $detail] = explode("\t", $entry);
        if ($kind === 'reached') {
            $out["$builtin\t\tprobed\t"] = true;
            continue;
        }
        if ($seenWhere !== $where || $kind === 'returned') {
            continue;
        }
        $out["$builtin\t$where\t$kind\t$detail"] = true;
    }
}
