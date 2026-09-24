<?php
            class Logger {
                /** @var resource|null */
                private mixed $fileHandle = null;
                public function __construct() {
                    $this->open();
                }
                private function open(): void {
                    $handle = fopen('/tmp/x', 'a');
                    if ($handle !== false) {
                        $this->fileHandle = $handle;
                    }
                }
                public function __destruct() {
                    if ($this->fileHandle !== null) {
                        fclose($this->fileHandle);
                    }
                }
            }
        