<?php
            use function PHPStan\Testing\assertType;
            /**
             * @return array{chronology_date?: list<string>, other?: string}
             */
            function parseIt(): array {
                return [];
            }
            function f(): void {
                /** @var array<string, mixed> */
                $page = [];
                if (rand(0, 1) === 1) {
                    $page['flat'] = true;
                } else {
                    $page['section'] = 'cats';
                }
                $page = array_merge($page, parseIt());
                if (isset($page['chronology_field'])) {
                    $calendar_date = is_array($page['chronology_date'] ?? null) ? $page['chronology_date'] : [];
                    assertType('list<string>', $calendar_date);
                }
            }
        