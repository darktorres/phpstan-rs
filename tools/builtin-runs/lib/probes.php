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
    $probes = [];
    $sized = known_parameter_value($param);
    $untyped = $type === null || (string) $type === 'mixed';
    $native = $sized === null;
    // A parameter with a valid value of its own is not given the kinds that a native call of it cannot take: arrays
    // of them, which phpredis and others crash or hang on, and, for an untyped one, callables and iterators.
    if ((admits($type, 'callable') || admits($type, 'closure')) && ($native || !$untyped)) {
        $probes[] = ['callable', static fn () => recording_callable()];
    }
    if (accepts_value($type, RecordingObject::class)) {
        $probes[] = ['object', static fn () => new RecordingObject()];
    }
    if (accepts_value($type, RecordingIterator::class) && ($native || !$untyped)) {
        $probes[] = ['iterator', static fn () => new RecordingIterator()];
    }
    if (accepts_value($type, RecordingSleeper::class) && ($native || !$untyped)) {
        $probes[] = ['sleeper', static fn () => new RecordingSleeper()];
    }
    // The length or int a sodium function checks the parameter against: the valid value of the parameter.
    // A string probe is refused by the length check of a sized parameter, so it is offered only where no sized value is known.
    if ($sized !== null) {
        $probes[] = ['sized', $sized];
    } elseif (admits($type, 'string')) {
        $probes[] = ['url', static fn () => 'rec://probe'];
    }
    // A class the parameter takes has a valid value (a DOM document, a SimpleXML element): that is a probe too.
    $valid = null;
    foreach ($type instanceof ReflectionUnionType ? $type->getTypes() : [$type] as $member) {
        if ($member instanceof ReflectionNamedType && !$member->isBuiltin() && ($valid = valid_value($member)) !== null) {
            break;
        }
    }
    if ($valid !== null) {
        $probes[] = ['valid', $valid];
    }
    // A scalar parameter takes a valid scalar, which the plain value of the parameter is.
    if (admits($type, 'int') || admits($type, 'float') || admits($type, 'bool')) {
        $probes[] = ['scalar', static fn () => plain_value($param)];
    }
    if (admits($type, 'array') && $native) {
        $probes[] = ['objects', static fn () => [new RecordingObject(), new RecordingObject()]];
        $probes[] = ['callables', static fn () => [recording_callable(), recording_callable()]];
        $probes[] = ['urls', static fn () => ['rec://probe']];
    }
    if (admits($type, 'resource') || $type === null || (string) $type === 'mixed') {
        $probes[] = ['stream', static fn () => recording_stream()];
    }
    // The stubs document a resource here, and PHP takes a stream or a stream context:
    // the context's notification callback is user code. Other kinds stay probed, since
    // the stubs can be out of date (PostgreSQL's connection also takes a string).
    if (documented_resource($param)) {
        $probes[] = ['context', static fn () => recording_context()];
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
        // A parser whose handlers are methods of an object: xml_set_object is what the handler setters require.
        'xmlparser' => static function (): XMLParser {
            $parser = xml_parser_create();
            xml_set_object($parser, new stdClass());
            return $parser;
        },
        // Not connected: a call on it fails for want of a connection, which the table records as a gap.
        // Connected to the MySQL server the scan starts inside its sandbox, on a Unix socket.
        'mysqli' => static fn (): mysqli => new mysqli(null, 'root', '', '', 0, server_run_dir() . '/mysql.sock'),
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
        'simplexmlelement' => static fn (): SimpleXMLElement => new SimpleXMLElement('<a><b/></a>'),
        'intlgregoriancalendar' => static fn (): IntlGregorianCalendar => IntlGregorianCalendar::createInstance('UTC', 'en'),
        'ds\\map' => static fn (): Ds\Map => new Ds\Map(),
        'sqlite3' => static fn (): SQLite3 => new SQLite3(':memory:'),
        // Connected to the PostgreSQL server the scan starts inside its sandbox, on a Unix socket.
        'pgsql\\connection' => static fn (): PgSql\Connection => pg_connect('host=' . server_run_dir() . ' port=5432 user=postgres dbname=postgres'),
        // Logged in to the FTP server the scan starts inside its sandbox, on loopback.
        'ftp\\connection' => static function (): FTP\Connection {
            $connection = ftp_connect('127.0.0.1', 2121);
            ftp_login($connection, 'anonymous', 'probe@example.com');
            return $connection;
        },
        // A large object, opened inside a transaction on the server the scan starts.
        'pgsql\\lob' => static function (): PgSql\Lob {
            $connection = pg_connect('host=' . server_run_dir() . ' port=5432 user=postgres dbname=postgres');
            pg_query($connection, 'BEGIN');
            return pg_lo_open($connection, pg_lo_create($connection), 'w+');
        },
        'pgsql\\result' => static fn (): PgSql\Result => pg_query(pg_connect('host=' . server_run_dir() . ' port=5432 user=postgres dbname=postgres'), 'SELECT 1'),
        'imagickkernel' => static fn (): ImagickKernel => ImagickKernel::fromBuiltin(Imagick::KERNEL_DIAMOND, '1'),
        'traversable' => static fn (): ArrayIterator => new ArrayIterator([1, 2]),
        'domelement' => static function (): DOMElement {
            $document = new DOMDocument();
            $document->loadXML('<a/>');
            return $document->documentElement;
        },
        'domnode' => static function (): DOMNode {
            $document = new DOMDocument();
            $document->loadXML('<a/>');
            return $document->documentElement;
        },
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

/** A document loaded from XML, as a DOM document. */
function dom_document_sample(string $xml): DOMDocument
{
    $document = new DOMDocument();
    $document->loadXML($xml);
    return $document;
}

/** An object with lazy initialisation, as a ReflectionClass of stdClass makes. */
function lazy_object_sample(): object
{
    return (new ReflectionClass(stdClass::class))->newLazyGhost(static function (): void {});
}

/** One end of a connected socket pair: a socket that select can wait on. */
function socket_pair_sample(): Socket
{
    socket_create_pair(AF_UNIX, SOCK_STREAM, 0, $pair);
    return $pair[0];
}

/** One end of a connected stream pair: a stream that stream_select can wait on. */
function stream_pair_sample(): mixed
{
    return stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP)[0];
}

/**
 * The directory the servers' Unix sockets are in: the worker's own run directory when the environment names one,
 * otherwise the temporary directory of the scan's sandbox.
 */
function server_run_dir(): string
{
    $dir = getenv('BUILTIN_RUNS_RUN');
    return $dir === false || $dir === '' ? '/tmp' : $dir;
}

/** A stream context whose notification callback is a recording callable. */
function recording_context(): mixed
{
    $context = stream_context_create();
    stream_context_set_params($context, ['notification' => recording_callable()]);
    Seen::$probeContext = $context;
    Seen::$probeUrl = listener_url();
    return $context;
}

/**
 * The URL of a listener on the loopback interface, which a forked process serves
 * with one HTTP response: a stream the notification callback of a context hears.
 * The process ends with SIGKILL, so the shutdown code of the child it came from
 * does not run in it.
 */
function listener_url(): string
{
    $server = stream_socket_server('tcp://127.0.0.1:0', $errno, $error);
    if ($server === false) {
        return 'http://127.0.0.1:1/';
    }
    $port = (int) substr(strrchr((string) stream_socket_get_name($server, false), ':'), 1);
    if (pcntl_fork() === 0) {
        $connection = @stream_socket_accept($server, 2);
        if ($connection !== false) {
            fread($connection, 4096);
            fwrite($connection, "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok");
            fclose($connection);
        }
        posix_kill(getmypid(), SIGKILL);
    }
    fclose($server);
    return "http://127.0.0.1:$port/";
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
/**
 * The valid value of a parameter that a probe of its own type gets wrong, from the families that need one: sodium
 * (a length, a state, an int) and mbstring (an encoding, a flag string). Null when the parameter has no such value.
 */
function known_parameter_value(ReflectionParameter $param): ?Closure
{
    return sodium_length_value($param) ?? mbstring_value($param) ?? bcmath_value($param) ?? hash_value($param) ?? uri_value($param) ?? openssl_value($param) ?? imagick_value($param) ?? pdo_value($param) ?? xml_value($param) ?? named_value($param);
}

/** The valid value of a bcmath parameter: a number string, a modulus that is not zero, or a scale. */
function bcmath_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction();
    $class = $function instanceof ReflectionMethod ? strtolower($function->getDeclaringClass()->getName()) : '';
    if (!str_starts_with(strtolower($function->getName()), 'bc') && $class !== 'bcmath\\number') {
        return null;
    }
    return match ($param->getName()) {
        'num', 'num1', 'num2' => static fn (): string => '1',
        'exponent' => static fn (): string => '2',
        'modulus' => static fn (): string => '3',
        'scale' => static fn (): int => 0,
        default => null,
    };
}

/** The valid value of a parameter that a function checks by name: a CSV character, an option array, a mode, a process. */
function named_value(ReflectionParameter $param): ?Closure
{
    $function = strtolower($param->getDeclaringFunction()->getName());
    $name = $param->getName();
    if (in_array($name, ['separator', 'enclosure', 'escape'], true) && in_array($function, ['str_getcsv', 'fgetcsv', 'fputcsv', 'setcsvcontrol', 'splfileobject::fgetcsv', 'splfileobject::fputcsv', 'splfileobject::setcsvcontrol'], true)) {
        return match ($name) {
            'separator' => static fn (): string => ',',
            'enclosure' => static fn (): string => '"',
            default => static fn (): string => '\\',
        };
    }
    // Methods of a class, by the class and the name.
    $class = $param->getDeclaringFunction() instanceof ReflectionMethod ? strtolower($param->getDeclaringFunction()->getDeclaringClass()->getName()) : '';
    if (str_starts_with($class, 'reflection') && $name === 'flags' && $function === 'getattributes') {
        return static fn (): int => 0;
    }
    if ($class === 'random\\randomizer' && in_array($name, ['min', 'max'], true)) {
        return $name === 'min' ? static fn (): float => 0.0 : static fn (): float => 1.0;
    }
    if ($class === 'spoofchecker' && $name === 'pattern') {
        return static fn (): string => '[a-z]';
    }
    // An XML handler is a callable: a string would name a method of the parser's object.
    if (str_starts_with($function, 'xml_set_') && str_ends_with($name, 'handler')) {
        return static fn (): Closure => static fn (): int => 0;
    }
    if (str_starts_with($function, 'image') && $name === 'file') {
        return static fn (): string => sys_get_temp_dir() . '/rec-' . getmypid() . '.png';
    }
    if (in_array($class, ['splfileobject', 'spltempfileobject'], true) && $name === 'context') {
        return static fn (): mixed => stream_context_create();
    }
    // PostgreSQL field and row numbers of a one-field, one-row result, flag masks, large-object modes and fetch classes.
    if (str_starts_with($function, 'pg_') || str_starts_with($function, 'pg') && str_contains($function, 'field')) {
        if (in_array($name, ['field', 'row'], true)) {
            return static fn (): int => 0;
        }
        if ($name === 'flags') {
            return static fn (): int => 0;
        }
        if ($name === 'mode' && str_contains($function, 'lo')) {
            return static fn (): string => 'r';
        }
        if ($name === 'class' && str_contains($function, 'fetch_object')) {
            return static fn (): string => 'stdClass';
        }
    }
    // A PostgreSQL function takes its connection first, from the server the scan starts.
    if (str_starts_with($function, 'pg_') && $name === 'connection') {
        return static fn (): PgSql\Connection => pg_connect('host=' . server_run_dir() . ' port=5432 user=postgres dbname=postgres');
    }
    // A database file of SQLite3 is in memory; a directory of the directory iterators is the temporary one.
    if ($class === 'sqlite3' && $name === 'filename') {
        return static fn (): string => ':memory:';
    }
    if (in_array($class, ['directoryiterator', 'filesystemiterator', 'recursivedirectoryiterator'], true) && $name === 'directory') {
        return static fn (): string => sys_get_temp_dir();
    }
    // MySQL arguments: the connection the scan starts inside its sandbox, a statement, a charset and a savepoint name.
    // A MySQL connection goes through the socket: without a host, and with port 0. A TCP attempt to the server, which
    // skips networking, fails and leaves the object half-open, and the next call on it crashes.
    if (str_starts_with($class, 'mysqli') || str_starts_with($function, 'mysqli')) {
        if ($name === 'hostname') {
            return static fn (): mixed => null;
        }
        $value = match ($name) {
            'query', 'sql' => 'SELECT 1',
            'port' => 0,
            'username' => 'root',
            'password', 'database' => '',
            'socket' => server_run_dir() . '/mysql.sock',
            'charset' => 'utf8mb4',
            'name' => 'sp1',
            default => null,
        };
        if ($value !== null) {
            return static fn (): mixed => $value;
        }
    }
    // A Redis option's value is an int: an object or a callable as the serializer breaks the next call that uses it.
    if ($class === 'redis' && $function === 'setoption' && $name === 'value') {
        return static fn (): int => Redis::SERIALIZER_NONE;
    }
    // Redis arguments with a valid value: SORT's pattern, offset, count and store; GEOSEARCH's and GEOSEARCHSTORE's
    // position and shape; and HSET's fields and values. A call with the array probes of these breaks the connection.
    if ($class === 'redis' && (str_starts_with($function, 'sort') || in_array($function, ['geosearch', 'geosearchstore', 'hset'], true))) {
        $value = match ($name) {
            'get' => 'x*',
            'offset' => 0,
            'count' => -1,
            'store' => 'dst',
            'position' => [0.0, 0.0],
            'shape' => 10.0,
            'fields_and_vals' => ['f' => 'v'],
            default => null,
        };
        if ($value !== null) {
            return static fn (): mixed => $value;
        }
    }
    // Methods of a date or timezone class, by the class and the name.
    if (in_array($class, ['datetime', 'datetimeimmutable'], true) && $name === 'datetime') {
        return static fn (): string => '2000-01-01';
    }
    if (in_array($class, ['datetime', 'datetimeimmutable'], true) && $name === 'modifier') {
        return static fn (): string => '+1 day';
    }
    if ($class === 'datetimezone' && $name === 'timezone') {
        return static fn (): string => 'UTC';
    }
    return match ([$function, $name]) {
        ['datefmt_create', 'locale'] => static fn (): string => 'en',
        ['checkdnsrr', 'type'], ['dns_check_record', 'type'] => static fn (): string => 'A',
        ['class_implements', 'object_or_class'], ['class_parents', 'object_or_class'], ['class_uses', 'object_or_class'] => static fn (): string => 'stdClass',
        ['closedir', 'dir_handle'], ['readdir', 'dir_handle'], ['rewinddir', 'dir_handle'] => static fn (): mixed => opendir(sys_get_temp_dir()),
        ['constant', 'name'] => static fn (): string => 'PHP_EOL',
        ['curl_setopt_array', 'options'] => static fn (): array => [CURLOPT_FOLLOWLOCATION => true],
        ['curl_share_init_persistent', 'share_options'] => static fn (): array => [CURL_LOCK_DATA_DNS],
        ['ftok', 'project_id'] => static fn (): string => 'a',
        ['get_resources', 'type'] => static fn (): string => 'stream',
        ['gzcompress', 'encoding'], ['gzdeflate', 'encoding'], ['gzencode', 'encoding'], ['inflate_init', 'encoding'] => static fn (): int => ZLIB_ENCODING_RAW,
        ['hash_init', 'key'] => static fn (): string => 'secret',
        ['jdtounix', 'julian_day'] => static fn (): int => 2440588,
        ['hash_init', 'flags'] => static fn (): int => 0,
        ['json_validate', 'flags'] => static fn (): int => 0,
        ['extract', 'prefix'] => static fn (): string => 'p',
        ['popen', 'mode'] => static fn (): string => 'r',
        ['transformtoxml', 'document'], ['transformtouri', 'document'], ['transformtodoc', 'document'] => static fn (): DOMDocument => dom_document_sample('<a/>'),
        ['importstylesheet', 'stylesheet'] => static fn (): DOMDocument => dom_document_sample('<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/"><xsl:copy-of select="."/></xsl:template></xsl:stylesheet>'),
        ['getattribute', 'name'] => static fn (): int => PDO::ATTR_DRIVER_NAME,
        ['getlazyinitializer', 'object'], ['initializelazyobject', 'object'], ['isuninitializedlazyobject', 'object'], ['marklazyobjectasinitialized', 'object'] => static fn (): object => lazy_object_sample(),
        ['socket_select', 'read'], ['socket_select', 'write'], ['socket_select', 'except'] => static fn (): array => [socket_pair_sample()],
        ['stream_select', 'read'], ['stream_select', 'write'], ['stream_select', 'except'] => static fn (): array => [stream_pair_sample()],
        // The callbacks form takes a callable first: a handler that records nothing.
        ['session_set_save_handler', 'open'] => static fn (): Closure => static fn (): bool => true,
        ['session_start', 'options'] => static fn (): array => ['cache_limiter' => 'nocache'],
        ['openfile', 'context'] => static fn (): mixed => stream_context_create(),
        ['openfile', 'mode'] => static fn (): string => 'r',
        ['transformtodoc', 'returnclass'] => static fn (): string => 'DOMDocument',
        ['substr_count', 'offset'] => static fn (): int => 0,
        ['resetaslazyghost', 'object'], ['resetaslazyproxy', 'object'] => static fn (): object => new stdClass(),
        ['simplexml_import_dom', 'class_name'], ['simplexml_load_string', 'class_name'], ['simplexml_load_file', 'class_name'] => static fn (): string => 'SimpleXMLElement',
        ['substr_count', 'length'] => static fn (): ?int => null,
        ['newlazyghost', 'options'], ['newlazyproxy', 'options'], ['resetaslazyghost', 'options'], ['resetaslazyproxy', 'options'] => static fn (): int => 0,
        ['simplexml_import_dom', 'node'] => static function (): DOMElement {
            $document = new DOMDocument();
            $document->loadXML('<a/>');
            return $document->documentElement;
        },
        ['php_uname', 'mode'] => static fn (): string => 'a',
        ['pack', 'format'] => static fn (): string => 'x',
        ['unpack', 'format'] => static fn (): string => 'C',
        ['version_compare', 'operator'] => static fn (): string => '>=',
        ['settype', 'type'] => static fn (): string => 'int',
        ['trigger_error', 'error_level'], ['user_error', 'error_level'] => static fn (): int => E_USER_NOTICE,
        ['c14n', 'xpath'], ['c14nfile', 'xpath'] => static fn (): array => ['query' => '/'],
        ['method_exists', 'object_or_class'], ['property_exists', 'object_or_class'] => static fn (): string => 'stdClass',
        ['union', 'map'] => static fn (): mixed => $class === 'ds\\map' ? new Ds\Map() : null,
        ['sscanf', 'format'], ['fscanf', 'format'], ['splfileobject', 'format'] => static fn (): string => '%d',
        ['class_implements', 'autoload'], ['class_parents', 'autoload'], ['class_uses', 'autoload'] => static fn (): bool => false,
        ['normalizer_normalize', 'form'], ['normalizer_is_normalized', 'form'] => static fn (): int => Normalizer::FORM_C,
        ['setfontweight', 'weight'] => static fn (): int => 400,
        ['bezier', 'coordinates'], ['polygon', 'coordinates'], ['polyline', 'coordinates'] => static fn (): array => [['x' => 0, 'y' => 0], ['x' => 1, 'y' => 0], ['x' => 0, 'y' => 1]],
        ['fetchcolumn', 'column'] => static fn (): int => 0,
        ['zlib_encode', 'encoding'] => static fn (): int => ZLIB_ENCODING_RAW,
        ['base_convert', 'from_base'] => static fn (): int => 16,
        ['base_convert', 'to_base'] => static fn (): int => 2,
        ['easter_date', 'year'], ['easter_days', 'year'] => static fn (): int => 2000,
        ['numfmt_create', 'locale'] => static fn (): string => 'en',
        ['xml_parser_create', 'encoding'], ['xml_parser_create_ns', 'encoding'] => static fn (): string => 'UTF-8',
        ['imagepolygon', 'num_points_or_color'], ['imagefilledpolygon', 'num_points_or_color'], ['imageopenpolygon', 'num_points_or_color'] => static fn (): int => 3,
        ['imagepolygon', 'points'], ['imagefilledpolygon', 'points'], ['imageopenpolygon', 'points'] => static fn (): array => [0, 0, 1, 0, 0, 1],
        ['imageaffinematrixconcat', 'matrix1'], ['imageaffinematrixconcat', 'matrix2'] => static fn (): array => [1, 0, 0, 1, 0, 0],
        ['imageaffine', 'affine'] => static fn (): array => [1, 0, 0, 1, 0, 0],
        ['imageaffine', 'clip'], ['imagecrop', 'rectangle'] => static fn (): array => ['x' => 0, 'y' => 0, 'width' => 1, 'height' => 1],
        ['imageconvolution', 'matrix'] => static fn (): array => [[1, 1, 1], [1, 1, 1], [1, 1, 1]],
        ['imagecolormatch', 'image2'] => static function (): GdImage {
            $palette = imagecreate(1, 1);
            imagecolorallocate($palette, 255, 255, 255);
            return $palette;
        },
        ['sodium_crypto_generichash_final', 'length'] => static fn (): int => 32,
        ['setrestrictionlevel', 'level'] => static fn (): int => Spoofchecker::SINGLE_SCRIPT_RESTRICTION,
        ['sodium_crypto_generichash', 'length'], ['sodium_crypto_generichash_init', 'length'] => static fn (): int => 32,
        ['str_increment', 'string'] => static fn (): string => 'a',
        ['str_decrement', 'string'] => static fn (): string => 'b',
        ['stream_context_create', 'options'], ['stream_context_get_default', 'options'], ['stream_context_set_default', 'options'], ['stream_context_set_options', 'options'] => static fn (): array => ['http' => ['method' => 'GET']],
        ['locale_lookup', 'languageTag'] => static fn (): array => ['en'],
        ['locale_compose', 'subtags'] => static fn (): array => ['language' => 'en'],
        ['intlcal_create_instance', 'timezone'], ['intlcal_set_time_zone', 'timezone'] => static fn (): string => 'UTC',
        ['openssl_seal', 'public_key'] => static fn (): array => [openssl_pkey_get_details(openssl_pkey_new(['private_key_bits' => 1024, 'private_key_type' => OPENSSL_KEYTYPE_RSA]))['key']],
        ['shmop_open', 'mode'] => static fn (): string => 'c',
        ['socket_addrinfo_lookup', 'hints'] => static fn (): array => ['ai_socktype' => SOCK_STREAM],
        ['preg_replace_callback_array', 'pattern'] => static fn (): array => ['/a/' => static fn (): string => 'x'],
        ['proc_close', 'process'], ['proc_get_status', 'process'], ['proc_terminate', 'process'] => static function (): mixed {
            return proc_open('true', [], $pipes);
        },
        default => null,
    };
}

/** The valid value of an XML name, or of a document that SimpleXML parses. */
function xml_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction();
    $class = $function instanceof ReflectionMethod ? strtolower($function->getDeclaringClass()->getName()) : '';
    $name = $param->getName();
    if ($name === 'data' && ($class === 'simplexmlelement' || $function->getName() === 'simplexml_load_string')) {
        return static fn (): string => '<a><b/></a>';
    }
    $xml = str_starts_with($class, 'xmlwriter') || str_starts_with($function->getName(), 'xmlwriter_');
    $dom = str_starts_with($class, 'dom') && !str_starts_with($class, 'domain');
    if (!$xml && !$dom) {
        return null;
    }
    if (in_array($name, ['name', 'localName', 'qualifiedName', 'target', 'namespace', 'content', 'prefix', 'value', 'isParam'], true)) {
        return static fn (): string => 'a';
    }
    return null;
}

/** A PNG image, as the bytes of a small one that Imagick writes. */
function imagick_blob_sample(): string
{
    $image = new Imagick();
    $image->newImage(2, 2, new ImagickPixel('white'));
    $image->setImageFormat('png');
    return $image->getImageBlob();
}

/** The valid value of an Imagick colour or a PDO fetch mode. */
function imagick_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction();
    $class = $function instanceof ReflectionMethod ? strtolower($function->getDeclaringClass()->getName()) : '';
    $name = $param->getName();
    if (!str_starts_with($class, 'imagick')) {
        return null;
    }
    // The SVG a DrawVectorGraphics call parses.
    if ($class === 'imagickdraw' && $name === 'xml') {
        return static fn (): string => '<svg/>';
    }
    // A channel of an ImagickPixel is an int, not a colour.
    if ($class === 'imagickpixel' && $name === 'color' && admits($param->getType(), 'int')) {
        return static fn (): int => Imagick::COLOR_RED;
    }
    // A colour: named so, or typed as ImagickPixel or a string.
    if (str_ends_with($name, 'color') || str_ends_with($name, 'colour') || str_contains((string) $param->getType(), 'ImagickPixel')) {
        return static fn (): string => 'white';
    }
    if ($name === 'geometry') {
        return static fn (): string => '1x1';
    }
    // Integers of a method on the sixteen-pixel image the recipes make: offsets at the origin, sizes that fit it.
    if ($class === 'imagick' && $name === 'map') {
        return static fn (): string => 'RGB';
    }
    if ($class === 'imagick' && in_array($name, ['x_resolution', 'y_resolution'], true)) {
        return static fn (): float => 72.0;
    }
    if ($class === 'imagick' && $name === 'pseudo_format') {
        return static fn (): string => 'xc:white';
    }
    if ($class === 'imagick' && $name === 'image' && in_array(strtolower($function->getName()), ['readimageblob', 'pingimageblob'], true)) {
        return static fn (): string => imagick_blob_sample();
    }
    if ($class === 'imagick' && !admits($param->getType(), 'string')) {
        return match ($name) {
            'x', 'y', 'left', 'top', 'offset', 'index' => static fn (): int => 0,
            'width', 'height', 'columns', 'rows' => static fn (): int => 4,
            'inner_bevel', 'outer_bevel', 'x_bevel', 'y_bevel' => static fn (): int => 1,
            default => null,
        };
    }
    // A file the image is written to, named with a format the image writers know.
    if ($name === 'filename' || $name === 'file') {
        return static fn (): string => sys_get_temp_dir() . '/rec-' . getmypid() . '.png';
    }
    return null;
}

/** The valid value of a PDO statement's fetch mode: an associative array. */
function pdo_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction();
    if (!$function instanceof ReflectionMethod || strtolower($function->getDeclaringClass()->getName()) !== 'pdostatement' || $param->getName() !== 'mode') {
        return null;
    }
    return static fn (): int => PDO::FETCH_ASSOC;
}

/** The valid value of an openssl key parameter: a PEM private or public key of a generated RSA pair. */
function openssl_value(ReflectionParameter $param): ?Closure
{
    // A parameter typed as a key object, or as an array of keys, takes that value instead of a PEM string.
    $type = (string) $param->getType();
    if (!str_starts_with($param->getDeclaringFunction()->getName(), 'openssl_') || str_contains($type, 'OpenSSLAsymmetricKey') || str_contains($type, 'array')) {
        return null;
    }
    return match ($param->getName()) {
        'private_key', 'key' => static function (): string {
            openssl_pkey_export(openssl_pkey_new(['private_key_bits' => 1024, 'private_key_type' => OPENSSL_KEYTYPE_RSA]), $pem);
            return $pem;
        },
        'public_key' => static function (): string {
            $pem = openssl_pkey_new(['private_key_bits' => 1024, 'private_key_type' => OPENSSL_KEYTYPE_RSA]);
            return openssl_pkey_get_details($pem)['key'];
        },
        default => null,
    };
}

/** The valid value of a URI class's parameter: an absolute URI, or a component of one. */
function uri_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction();
    if (!$function instanceof ReflectionMethod || !str_starts_with(strtolower($function->getDeclaringClass()->getName()), 'uri\\')) {
        return null;
    }
    return match ($param->getName()) {
        'uri' => static fn (): string => 'https://example.com/',
        'host' => static fn (): string => 'example.com',
        'scheme' => static fn (): string => 'https',
        'userinfo' => static fn (): string => 'user:pass',
        'username' => static fn (): string => 'user',
        'password' => static fn (): string => 'pass',
        'path' => static fn (): string => '/a',
        'query' => static fn (): string => 'b=1',
        'fragment' => static fn (): string => 'f',
        default => null,
    };
}

/** The valid value of a hash function's algorithm: a name the hash extension knows. */
function hash_value(ReflectionParameter $param): ?Closure
{
    if ($param->getName() !== 'algo' || !str_starts_with($param->getDeclaringFunction()->getName(), 'hash')) {
        return null;
    }
    return static fn (): string => 'sha256';
}

/** The valid value of an mbstring parameter: an encoding name, or the flags and types its options take. */
function mbstring_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction()->getName();
    if (!str_starts_with($function, 'mb_')) {
        return null;
    }
    $name = $param->getName();
    if (in_array($name, ['encoding', 'to_encoding', 'from_encoding', 'charset', 'encodings'], true)) {
        return static fn (): string => 'UTF-8';
    }
    // Conversion maps: groups of four ints (start, end, offset, mask).
    if ($name === 'map' && str_contains($function, 'numericentity')) {
        return static fn (): array => [0, 127, 0, 127];
    }
    // Options and flags of each function that the stubs take as a string.
    static $values = [
        'mb_convert_kana' => ['mode' => 'a'],
        'mb_http_input' => ['type' => 'G'],
        'mb_language' => ['language' => 'uni'],
        'mb_substitute_character' => ['substitute_character' => 'none'],
        'mb_encode_mimeheader' => ['transfer_encoding' => 'B', 'newline' => "\r\n"],
        'mb_ereg' => ['options' => ''],
        'mb_ereg_match' => ['options' => ''],
        'mb_ereg_replace' => ['options' => ''],
        'mb_ereg_replace_callback' => ['options' => ''],
        'mb_ereg_search_init' => ['options' => ''],
        'mb_eregi_replace' => ['options' => ''],
        'mb_regex_set_options' => ['options' => ''],
    ];
    $value = $values[$function][$name] ?? null;
    return $value === null ? null : static fn (): string => $value;
}

/**
 * The valid values of a sodium function's parameters that a probe of the parameter's own type gets wrong: a
 * string of the length the function checks, an int, or a state or key that the function's other calls make.
 * Lengths are keyed by function and parameter name, and each constant is named as the scan's messages write
 * it, with each run of digits as N; it is found by that name, so no constant's value is written out here.
 */
function sodium_length_value(ReflectionParameter $param): ?Closure
{
    $function = $param->getDeclaringFunction()->getName();
    $name = $param->getName();
    // Values the function's own family makes: a state from an init call, an encoded string, a sign key.
    static $values = [
        'sodium_add' => ['string1' => 'SAME', 'string2' => 'SAME'],
        'sodium_compare' => ['string1' => 'SAME', 'string2' => 'SAME'],
        'sodium_memcmp' => ['string1' => 'SAME', 'string2' => 'SAME'],
        'sodium_base642bin' => ['string' => 'BASE64'],
        'sodium_hex2bin' => ['string' => 'HEX'],
        'sodium_crypto_generichash_final' => ['state' => 'GENERICHASH_STATE'],
        'sodium_crypto_generichash_update' => ['state' => 'GENERICHASH_STATE'],
        'sodium_crypto_secretstream_xchacha20poly1305_pull' => ['state' => 'SECRETSTREAM_STATE'],
        'sodium_crypto_secretstream_xchacha20poly1305_rekey' => ['state' => 'SECRETSTREAM_STATE'],
        'sodium_crypto_secretstream_xchacha20poly1305_push' => ['state' => 'SECRETSTREAM_STATE'],
        'sodium_crypto_sign_ed25519_pk_to_curve25519' => ['public_key' => 'SIGN_PUBLIC'],
        'sodium_unpad' => ['string' => 'PADDED'],
    ];
    // Integers: a literal, or a constant named after `SODIUM_CRYPTO_`.
    static $integers = [
        'sodium_crypto_kdf_derive_from_key' => ['subkey_length' => 'KDF_BYTES_MIN'],
        'sodium_crypto_stream_xchacha20_xor_ic' => ['counter' => 0],
        'sodium_crypto_pwhash' => ['length' => 32, 'algo' => 'PWHASH_ALG_DEFAULT', 'opslimit' => 'PWHASH_OPSLIMIT_INTERACTIVE', 'memlimit' => 'PWHASH_MEMLIMIT_INTERACTIVE'],
        'sodium_crypto_pwhash_scryptsalsa208sha256' => ['length' => 32, 'opslimit' => 'PWHASH_SCRYPTSALSA208SHA256_OPSLIMIT_INTERACTIVE', 'memlimit' => 'PWHASH_SCRYPTSALSA208SHA256_MEMLIMIT_INTERACTIVE'],
        'sodium_unpad' => ['block_size' => 16],
        'sodium_crypto_pwhash_str' => ['opslimit' => 'PWHASH_OPSLIMIT_INTERACTIVE', 'memlimit' => 'PWHASH_MEMLIMIT_INTERACTIVE'],
        'sodium_crypto_pwhash_scryptsalsa208sha256_str' => ['opslimit' => 'PWHASH_SCRYPTSALSA208SHA256_OPSLIMIT_INTERACTIVE', 'memlimit' => 'PWHASH_SCRYPTSALSA208SHA256_MEMLIMIT_INTERACTIVE'],
    ];
    // Byte strings of the length a constant names.
    static $lengths = [
        'sodium_crypto_aead_aes256gcm_decrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_AESNGCM_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_AESNGCM_NPUBBYTES'],
        'sodium_crypto_aead_aes256gcm_encrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_AESNGCM_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_AESNGCM_NPUBBYTES'],
        'sodium_crypto_aead_chacha20poly1305_decrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_NPUBBYTES'],
        'sodium_crypto_aead_chacha20poly1305_encrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_NPUBBYTES'],
        'sodium_crypto_aead_chacha20poly1305_ietf_decrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_IETF_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_IETF_NPUBBYTES'],
        'sodium_crypto_aead_chacha20poly1305_ietf_encrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_IETF_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_CHACHANPOLYN_IETF_NPUBBYTES'],
        'sodium_crypto_aead_xchacha20poly1305_ietf_decrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_XCHACHANPOLYN_IETF_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_XCHACHANPOLYN_IETF_NPUBBYTES'],
        'sodium_crypto_aead_xchacha20poly1305_ietf_encrypt' => ['key' => 'SODIUM_CRYPTO_AEAD_XCHACHANPOLYN_IETF_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_AEAD_XCHACHANPOLYN_IETF_NPUBBYTES'],
        'sodium_crypto_auth' => ['key' => 'SODIUM_CRYPTO_AUTH_KEYBYTES'],
        'sodium_crypto_auth_verify' => ['key' => 'SODIUM_CRYPTO_AUTH_KEYBYTES', 'mac' => 'SODIUM_CRYPTO_AUTH_BYTES'],
        'sodium_crypto_box' => ['key_pair' => 'SODIUM_CRYPTO_BOX_KEYPAIRBYTES', 'nonce' => 'SODIUM_CRYPTO_BOX_NONCEBYTES'],
        'sodium_crypto_box_keypair_from_secretkey_and_publickey' => ['public_key' => 'SODIUM_CRYPTO_BOX_PUBLICKEYBYTES', 'secret_key' => 'SODIUM_CRYPTO_BOX_SECRETKEYBYTES'],
        'sodium_crypto_box_open' => ['key_pair' => 'SODIUM_CRYPTO_BOX_KEYPAIRBYTES', 'nonce' => 'SODIUM_CRYPTO_BOX_NONCEBYTES'],
        'sodium_crypto_box_publickey' => ['key_pair' => 'SODIUM_CRYPTO_BOX_KEYPAIRBYTES'],
        'sodium_crypto_box_publickey_from_secretkey' => ['secret_key' => 'SODIUM_CRYPTO_BOX_SECRETKEYBYTES'],
        'sodium_crypto_box_seal' => ['public_key' => 'SODIUM_CRYPTO_BOX_PUBLICKEYBYTES'],
        'sodium_crypto_box_seal_open' => ['key_pair' => 'SODIUM_CRYPTO_BOX_KEYPAIRBYTES'],
        'sodium_crypto_box_secretkey' => ['key_pair' => 'SODIUM_CRYPTO_BOX_KEYPAIRBYTES'],
        'sodium_crypto_box_seed_keypair' => ['seed' => 'SODIUM_CRYPTO_BOX_SEEDBYTES'],
        'sodium_crypto_core_ristretto255_add' => ['p' => 'SODIUM_CRYPTO_CORE_RISTRETTON_BYTES', 'q' => 'SODIUM_CRYPTO_CORE_RISTRETTON_BYTES'],
        'sodium_crypto_core_ristretto255_from_hash' => ['s' => 'SODIUM_CRYPTO_CORE_RISTRETTON_HASHBYTES'],
        'sodium_crypto_core_ristretto255_is_valid_point' => ['s' => 'SODIUM_CRYPTO_CORE_RISTRETTON_BYTES'],
        'sodium_crypto_core_ristretto255_scalar_add' => ['x' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES', 'y' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_core_ristretto255_scalar_complement' => ['s' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_core_ristretto255_scalar_invert' => ['s' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_core_ristretto255_scalar_mul' => ['x' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES', 'y' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_core_ristretto255_scalar_negate' => ['s' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_core_ristretto255_scalar_reduce' => ['s' => 'SODIUM_CRYPTO_CORE_RISTRETTON_NONREDUCEDSCALARBYTES'],
        'sodium_crypto_core_ristretto255_scalar_sub' => ['x' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES', 'y' => 'SODIUM_CRYPTO_CORE_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_core_ristretto255_sub' => ['p' => 'SODIUM_CRYPTO_CORE_RISTRETTON_BYTES', 'q' => 'SODIUM_CRYPTO_CORE_RISTRETTON_BYTES'],
        'sodium_crypto_generichash' => ['key' => 'SODIUM_CRYPTO_GENERICHASH_KEYBYTES'],
        'sodium_crypto_generichash_init' => ['key' => 'SODIUM_CRYPTO_GENERICHASH_KEYBYTES'],
        'sodium_crypto_kdf_derive_from_key' => ['context' => 'SODIUM_CRYPTO_KDF_CONTEXTBYTES', 'key' => 'SODIUM_CRYPTO_KDF_KEYBYTES'],
        'sodium_crypto_kx_client_session_keys' => ['client_key_pair' => 'SODIUM_CRYPTO_KX_KEYPAIRBYTES', 'server_key' => 'SODIUM_CRYPTO_KX_PUBLICKEYBYTES'],
        'sodium_crypto_kx_publickey' => ['key_pair' => 'SODIUM_CRYPTO_KX_KEYPAIRBYTES'],
        'sodium_crypto_kx_secretkey' => ['key_pair' => 'SODIUM_CRYPTO_KX_KEYPAIRBYTES'],
        'sodium_crypto_kx_seed_keypair' => ['seed' => 'SODIUM_CRYPTO_KX_SEEDBYTES'],
        'sodium_crypto_kx_server_session_keys' => ['client_key' => 'SODIUM_CRYPTO_KX_PUBLICKEYBYTES', 'server_key_pair' => 'SODIUM_CRYPTO_KX_KEYPAIRBYTES'],
        'sodium_crypto_pwhash' => ['salt' => 'SODIUM_CRYPTO_PWHASH_SALTBYTES'],
        'sodium_crypto_pwhash_scryptsalsa208sha256' => ['salt' => 'SODIUM_CRYPTO_PWHASH_SCRYPTSALSANSHAN_SALTBYTES'],
        'sodium_crypto_scalarmult' => ['n' => 'SODIUM_CRYPTO_SCALARMULT_SCALARBYTES', 'p' => 'SODIUM_CRYPTO_SCALARMULT_BYTES'],
        'sodium_crypto_scalarmult_base' => ['secret_key' => 'SODIUM_CRYPTO_BOX_SECRETKEYBYTES'],
        'sodium_crypto_scalarmult_ristretto255' => ['n' => 'SODIUM_CRYPTO_SCALARMULT_RISTRETTON_SCALARBYTES', 'p' => 'SODIUM_CRYPTO_SCALARMULT_RISTRETTON_BYTES'],
        'sodium_crypto_scalarmult_ristretto255_base' => ['n' => 'SODIUM_CRYPTO_SCALARMULT_RISTRETTON_SCALARBYTES'],
        'sodium_crypto_secretbox' => ['key' => 'SODIUM_CRYPTO_SECRETBOX_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_SECRETBOX_NONCEBYTES'],
        'sodium_crypto_secretbox_open' => ['key' => 'SODIUM_CRYPTO_SECRETBOX_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_SECRETBOX_NONCEBYTES'],
        'sodium_crypto_secretstream_xchacha20poly1305_init_pull' => ['header' => 'SODIUM_CRYPTO_SECRETSTREAM_XCHACHANPOLYN_HEADERBYTES', 'key' => 'SODIUM_CRYPTO_SECRETSTREAM_XCHACHANPOLYN_KEYBYTES'],
        'sodium_crypto_secretstream_xchacha20poly1305_init_push' => ['key' => 'SODIUM_CRYPTO_SECRETSTREAM_XCHACHANPOLYN_KEYBYTES'],
        'sodium_crypto_shorthash' => ['key' => 'SODIUM_CRYPTO_SHORTHASH_KEYBYTES'],
        'sodium_crypto_sign' => ['secret_key' => 'SODIUM_CRYPTO_SIGN_SECRETKEYBYTES'],
        'sodium_crypto_sign_detached' => ['secret_key' => 'SODIUM_CRYPTO_SIGN_SECRETKEYBYTES'],
        'sodium_crypto_sign_ed25519_pk_to_curve25519' => ['public_key' => 'SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES'],
        'sodium_crypto_sign_ed25519_sk_to_curve25519' => ['secret_key' => 'SODIUM_CRYPTO_SIGN_SECRETKEYBYTES'],
        'sodium_crypto_sign_keypair_from_secretkey_and_publickey' => ['public_key' => 'SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES', 'secret_key' => 'SODIUM_CRYPTO_SIGN_SECRETKEYBYTES'],
        'sodium_crypto_sign_open' => ['public_key' => 'SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES'],
        'sodium_crypto_sign_publickey' => ['key_pair' => 'SODIUM_CRYPTO_SIGN_KEYPAIRBYTES'],
        'sodium_crypto_sign_publickey_from_secretkey' => ['secret_key' => 'SODIUM_CRYPTO_SIGN_SECRETKEYBYTES'],
        'sodium_crypto_sign_secretkey' => ['key_pair' => 'SODIUM_CRYPTO_SIGN_KEYPAIRBYTES'],
        'sodium_crypto_sign_seed_keypair' => ['seed' => 'SODIUM_CRYPTO_SIGN_SEEDBYTES'],
        'sodium_crypto_sign_verify_detached' => ['public_key' => 'SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES', 'signature' => 'SODIUM_CRYPTO_SIGN_BYTES'],
        'sodium_crypto_stream' => ['key' => 'SODIUM_CRYPTO_STREAM_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_STREAM_NONCEBYTES'],
        'sodium_crypto_stream_xchacha20' => ['key' => 'SODIUM_CRYPTO_STREAM_XCHACHAN_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_STREAM_XCHACHAN_NONCEBYTES'],
        'sodium_crypto_stream_xchacha20_xor' => ['key' => 'SODIUM_CRYPTO_STREAM_XCHACHAN_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_STREAM_XCHACHAN_NONCEBYTES'],
        'sodium_crypto_stream_xchacha20_xor_ic' => ['key' => 'SODIUM_CRYPTO_STREAM_XCHACHAN_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_STREAM_XCHACHAN_NONCEBYTES'],
        'sodium_crypto_stream_xor' => ['key' => 'SODIUM_CRYPTO_STREAM_KEYBYTES', 'nonce' => 'SODIUM_CRYPTO_STREAM_NONCEBYTES'],
    ];
    $value = $values[$function][$name] ?? null;
    if ($value !== null) {
        return match ($value) {
            'SAME' => static fn (): string => str_repeat('x', strlen('rec://probe')),
            'BASE64' => static fn (): string => base64_encode('xx'),
            'HEX' => static fn (): string => bin2hex('xx'),
            'GENERICHASH_STATE' => static fn (): string => sodium_crypto_generichash_init(),
            'SECRETSTREAM_STATE' => static fn (): string => sodium_crypto_secretstream_xchacha20poly1305_init_push(sodium_crypto_secretstream_xchacha20poly1305_keygen())[0],
            'SIGN_PUBLIC' => static fn (): string => sodium_crypto_sign_publickey(sodium_crypto_sign_keypair()),
            'PADDED' => static fn (): string => sodium_pad('x', 16),
        };
    }
    $integer = $integers[$function][$name] ?? null;
    if ($integer !== null) {
        return static fn (): int => is_int($integer) ? $integer : (int) constant('SODIUM_CRYPTO_' . $integer);
    }
    $constant = $lengths[$function][$name] ?? null;
    if ($constant === null) {
        return null;
    }
    return static function () use ($constant): string {
        foreach (get_defined_constants(true)['sodium'] ?? [] as $named => $length) {
            if (preg_replace('/\\d+/', 'N', $named) !== $constant) {
                continue;
            }
            // A Ristretto point or scalar is valid only when made from a hash or reduced, not from random bytes.
            if (str_ends_with($constant, 'RISTRETTON_BYTES')) {
                return sodium_crypto_core_ristretto255_from_hash(random_bytes(64));
            }
            if (str_ends_with($constant, 'RISTRETTON_SCALARBYTES')) {
                return sodium_crypto_core_ristretto255_scalar_reduce(random_bytes(64));
            }
            return random_bytes($length);
        }
        throw new RuntimeException("no sodium constant $constant");
    };
}

function valid_site(ReflectionParameter $param): ?Closure
{
    if ($length = known_parameter_value($param)) {
        return $length;
    }
    // Each value is a closure, so a constant of an extension that is not loaded is read only when its function is probed.
    static $sites = [
        'mb_convert_variables' => [
            0 => 'UTF-8',
            1 => 'UTF-8',
        ],
        'curl_setopt' => [1 => static fn (): int => CURLOPT_FOLLOWLOCATION],
        'curl_multi_setopt' => [1 => static fn (): int => CURLMOPT_MAXCONNECTS],
        'curl_share_setopt' => [1 => static fn (): int => CURLSHOPT_SHARE],
        'datefmt_create' => [0 => 'en', 3 => 'UTC'],
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
        'fopen' => [1 => 'r', 3 => static fn (): mixed => stream_context_create()],
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
        // The array form when the probe is the array; the string form when the probe comes after it.
        'strtr' => [1 => static fn (int $probe = -1): mixed => $probe >= 2 ? 'a' : ['a' => 'b']],
        // Callables that record nothing: a recording one would be attributed to whichever probe is running.
        'array_udiff_uassoc' => [1 => static fn (): array => [[1, 2], static fn (): int => 0, static fn (): int => 0]],
        'array_uintersect_uassoc' => [1 => static fn (): array => [[1, 2], static fn (): int => 0, static fn (): int => 0]],
        'session_set_save_handler' => [1 => static fn (): mixed => static fn (): int => 0],
        'stream_copy_to_stream' => [0 => static fn (): mixed => fopen('php://memory', 'r+'), 1 => static fn (): mixed => fopen('php://memory', 'r+')],
        'sodium_crypto_secretstream_xchacha20poly1305_push' => [0 => static fn (): string => sodium_crypto_secretstream_xchacha20poly1305_init_push(sodium_crypto_secretstream_xchacha20poly1305_keygen())[0]],
        'array_multisort' => [0 => static fn (): array => [2, 1]],
        'imagick::newimage' => [2 => 'white'],
        'sodium_crypto_generichash_final' => [0 => static fn (): string => sodium_crypto_generichash_init()],
        'sodium_crypto_generichash_update' => [0 => static fn (): string => sodium_crypto_generichash_init()],
        'sodium_crypto_secretstream_xchacha20poly1305_pull' => [0 => static function (): string {
            $key = sodium_crypto_secretstream_xchacha20poly1305_keygen();
            [, $header] = sodium_crypto_secretstream_xchacha20poly1305_init_push($key);
            return sodium_crypto_secretstream_xchacha20poly1305_init_pull($header, $key);
        }],
        'sodium_crypto_secretstream_xchacha20poly1305_rekey' => [0 => static fn (): string => sodium_crypto_secretstream_xchacha20poly1305_init_push(sodium_crypto_secretstream_xchacha20poly1305_keygen())[0]],
        // A no-op command and no descriptors: proc_open starts a process whatever it is given.
        'proc_open' => [0 => 'true', 1 => static fn (): array => []],
        // A null connection reaches the object argument's __toString, which a stream connection does not.
        'pg_lo_open' => [0 => static fn (): null => null],
        'pg_lo_export' => [0 => static fn (): null => null],
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
    return $value instanceof Closure ? $value : static fn (int $probe = -1): mixed => $value;
}

/** A plain value for a parameter a probe is not at. */
function plain_value(ReflectionParameter $param, int $probe = -1): mixed
{
    $site = valid_site($param);
    if ($site !== null) {
        return $site($probe);
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
function arguments(array $params, int $at, mixed $probe, bool $optional = false): array
{
    $args = [];
    // The context probe's call reads the listener: its first string argument is the URL.
    $listen = Seen::$probeUrl !== null && $probe === Seen::$probeContext;
    foreach ($params as $i => $param) {
        if ($i === $at) {
            $args[$i] = $probe;
        } elseif ($listen && declares_string($param)) {
            $args[$i] = Seen::$probeUrl;
            $listen = false;
        } elseif ($i > $at && $param->isVariadic()) {
            // Only a site value that lists the variadic values adds any: otherwise it stays out, as before.
            if (valid_site($param) !== null) {
                foreach (plain_value($param, $at) as $value) {
                    $args[] = $value;
                }
            }
        } elseif ($i > $at && $param->isOptional()) {
            // A parameter with a valid value of its own is passed with it. Otherwise it is left out unless the call
            // was refused for too few arguments (see invoke_arguments).
            $own = own_value($param);
            if ($own !== null) {
                $args[$i] = $own($at);
            } elseif (!$optional) {
                break;
            } else {
                $args[$i] = $param->isDefaultValueAvailable() ? $param->getDefaultValue() : plain_value($param, $at);
            }
        } else {
            $args[$i] = plain_value($param, $at);
        }
    }
    return $args;
}

/** The valid value of a parameter of its own: from its site, else from its named rule. Null when it has neither. */
function own_value(ReflectionParameter $param): ?Closure
{
    return valid_site($param) ?? known_parameter_value($param);
}

/** Whether a parameter is declared as a string: `string`, or a union with it (an untyped parameter is not). */
function declares_string(ReflectionParameter $param): bool
{
    $type = $param->getType();
    if ($type instanceof ReflectionNamedType) {
        return $type->getName() === 'string';
    }
    if ($type instanceof ReflectionUnionType) {
        foreach ($type->getTypes() as $member) {
            if ($member instanceof ReflectionNamedType && $member->getName() === 'string') {
                return true;
            }
        }
    }
    return false;
}

/**
 * Calls with the arguments of a probe. A call PHP refuses for too few arguments is made again with
 * the optional parameters after the probe as well. PHP raises that error before any body runs, so
 * the refused call records nothing the second one would not.
 */
function invoke_arguments(array $params, int $at, mixed $probe, Closure $call): mixed
{
    try {
        return $call(arguments($params, $at, $probe));
    } catch (ArgumentCountError) {
        return $call(arguments($params, $at, $probe, true));
    }
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
    // A trace, when the run names one: each child's result is appended as it is known, so a run can be followed.
    $trace = getenv('BUILTIN_RUNS_TRACE');
    if ($trace !== false && $trace !== '') {
        file_put_contents($trace, json_encode(['builtin' => $builtin, 'where' => $where, 'probe' => Stats::$probe, 'result' => $seen === null ? 'no-result' : 'ok'], JSON_UNESCAPED_SLASHES) . "\n", FILE_APPEND);
    }
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
