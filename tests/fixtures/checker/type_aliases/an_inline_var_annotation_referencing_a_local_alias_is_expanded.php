<?php
            /**
             * @phpstan-type ComponentMap array{scheme: ?string, host: ?string}
             */
            class UriString {
                public function parse(): void {
                    /** @var ComponentMap $components */
                    $components = ['scheme' => null, 'host' => null];
                    $missing = $components['nope'];
                }
            }
        