<?php
            /**
             * @template-covariant T
             */
            interface TypeInterface {
                /** @return T */
                public function coerce(mixed $value): mixed;
            }
            /**
             * @template T of array-key
             */
            final class SetType {
                /** @param TypeInterface<T> $type */
                public function __construct(private TypeInterface $type) {}

                public function coerce(mixed $value): void {
                    /** @var TypeInterface<T> $type */
                    $type = $this->type;
                    $v = $type->coerce($value);
                    $set = [];
                    $set[$v] = $v;
                }
            }
        