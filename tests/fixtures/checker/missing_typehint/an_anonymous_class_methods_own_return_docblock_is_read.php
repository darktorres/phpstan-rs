<?php
            function make(): object {
                return new class {
                    /**
                     * @return array<string, object>
                     */
                    public function getConnections(): array {
                        return [];
                    }
                };
            }
        