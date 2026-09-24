<?php
            function make() {
                return new class {
                    public function self(): static {
                        return $this;
                    }
                };
            }
        