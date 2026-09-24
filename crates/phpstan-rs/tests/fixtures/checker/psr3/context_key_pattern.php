<?php
namespace Psr\Log {
    interface LoggerInterface {
        public function log($level, string|\Stringable $message, array $context = []): void;
        public function error(string|\Stringable $message, array $context = []): void;
        public function info(string|\Stringable $message, array $context = []): void;
    }
}
namespace App {
use Psr\Log\LoggerInterface;
function f(LoggerInterface $logger): void {
    $logger->info('ctx', ['user_id' => 1, 'userId' => 2]);
}
}
