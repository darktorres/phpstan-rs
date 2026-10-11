<?php
// A worker of the recorder: a PHP process that the orchestrator (record.php)
// starts and sends units to, one at a time, on the protocol of lib/protocol.php.
// It ends when its input closes. Each call runs in a child of its own
// (lib/child.php), in a directory of its own (lib/workdir.php).

error_reporting(0);
ini_set('display_errors', '0');
ini_set('memory_limit', '512M');

require_once __DIR__ . '/lib/seen.php';
require_once __DIR__ . '/lib/probes.php';
require_once __DIR__ . '/lib/child.php';
require_once __DIR__ . '/lib/plan.php';
require_once __DIR__ . '/lib/unit.php';
require_once __DIR__ . '/lib/protocol.php';
require_once __DIR__ . '/lib/workdir.php';
require_once __DIR__ . '/lib/stats.php';
require_once __DIR__ . '/lib/select.php';

// Processes that are not dumpable never start the system's crash handler,
// which costs seconds for each abort. The setting is inherited by the children.
if (extension_loaded('ffi')) {
    try {
        FFI::cdef('int prctl(int option, unsigned long a, unsigned long b, unsigned long c, unsigned long d);', 'libc.so.6')
            ->prctl(4, 0, 0, 0, 0);
    } catch (Throwable) {
    }
}

Protocol::open();
Workdir::create();

$wanted = selector(null, null);
$skip = 'off';
try {
    while (($message = receive(Protocol::$in)) !== null && is_array($message)) {
        switch ($message[0]) {
            case 'init':
                Stats::$enabled = $message[2];
                $skip = $message[3];
                $wanted = selector($message[1], $message[4]);
                send(Protocol::$out, ['ready', manifest()]);
                break;
            case 'plan':
                $units = units($wanted);
                $nothing = [];
                foreach (RUNS_NOTHING as $function) {
                    if (function_exists($function) && $wanted($function)) {
                        $nothing[] = $function;
                    }
                }
                $signatures = [];
                foreach ($units as $unit) {
                    $signatures[] = unit_signature($unit);
                }
                send(Protocol::$out, ['plan', $units, unit_weights($units, $wanted), $nothing, $signatures]);
                break;
            case 'unit':
                $lines = run_unit($message[1], $wanted, $skip, $message[2]);
                send(Protocol::$out, ['done', $lines, Stats::payload()]);
                break;
        }
    }
} catch (RuntimeException) {
}
Workdir::remove();
exit(0);
